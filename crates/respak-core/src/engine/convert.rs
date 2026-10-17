//! `Engine::convert`: expands every output's sources and runs the conversions.

use std::sync::atomic::{AtomicU64, Ordering};

use super::request::{with_extension, without_extension};
use super::{
    ConvertReport, ConvertRequest, Engine, OutputReport, OutputRequest, OutputResult, SourceSpec,
};
use crate::error::{Error, ErrorKind, Result};
use crate::event::Event;
use crate::mediator::{Cx, Target};
use crate::value::{BundleData, FormatId};
use crate::CancellationToken;

impl Engine {
    /// Runs every output, up to `max_concurrency` at a time; one failing output does not stop
    /// the others. A glob or a list in `from` gives one report entry per conversion.
    /// `Err` only for an invalid request (unknown `version`).
    pub async fn convert(
        &self,
        request: &ConvertRequest,
        cancel: &CancellationToken,
    ) -> Result<ConvertReport> {
        if request.version != 1 {
            return Err(Error::new(ErrorKind::InvalidRequest {
                message: format!("unknown request version {}", request.version),
            }));
        }
        let cx = self.cx(cancel);

        // Expand every `from` first, in request order, so the report has a stable order.
        let mut jobs: Vec<Job<'_>> = Vec::new();
        for out in &request.outputs {
            let sources = out.from.sources();
            if sources.is_empty() {
                let error = Error::new(ErrorKind::InvalidRequest {
                    message: "`from` is an empty list".into(),
                });
                jobs.push(Job {
                    from: out.from.label(),
                    format: None,
                    out,
                    error: Some(error),
                });
            }
            for source in sources {
                let (pattern, format) = match source {
                    SourceSpec::Path(p) => (p.as_str(), None),
                    SourceSpec::As { path, format } => (path.as_str(), Some(format)),
                };
                match self.mediator.expand(pattern, &cx).await {
                    Ok(paths) => {
                        jobs.extend(paths.into_iter().map(|from| Job {
                            from,
                            format,
                            out,
                            error: None,
                        }));
                    }
                    Err(error) => jobs.push(Job {
                        from: pattern.to_owned(),
                        format,
                        out,
                        error: Some(error),
                    }),
                }
            }
        }

        use futures::StreamExt;
        let (total, done) = (jobs.len() as u64, AtomicU64::new(0));
        let outputs = futures::stream::iter(jobs)
            .map(|job| {
                let (cx, done) = (&cx, &done);
                async move {
                    let scoped = cx.scoped();
                    let result = match job.error {
                        Some(error) => Err(error),
                        None => {
                            self.convert_one(job.out, &job.from, job.format, &scoped)
                                .await
                        }
                    };
                    let skip = job.out.skip_unsupported
                        && matches!(&result, Err(e) if matches!(e.kind(), ErrorKind::NoPath { .. }));
                    let result = match result {
                        Ok(files) => OutputResult::Ok {
                            files,
                            dependencies: scoped.dependencies(),
                        },
                        Err(error) => OutputResult::Err { error },
                    };
                    cx.mediator.publish(Event::Progress {
                        task: "convert".into(),
                        done: done.fetch_add(1, Ordering::SeqCst) + 1,
                        total,
                    });
                    (
                        skip,
                        OutputReport {
                            from: job.from,
                            result,
                        },
                    )
                }
            })
            .buffered(self.max_concurrency)
            .filter_map(|(skip, report)| futures::future::ready((!skip).then_some(report)))
            .collect()
            .await;
        Ok(ConvertReport { outputs })
    }

    async fn convert_one(
        &self,
        out: &OutputRequest,
        from: &str,
        format: Option<&FormatId>,
        cx: &Cx,
    ) -> Result<Vec<String>> {
        cx.check_cancelled()?;
        let mut source = self.mediator.open(from, cx).await?;
        if let Some(format) = format {
            source = self.mediator.identify_as(source, format, cx).await?;
        }
        let options = if out.to.options.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Object(out.to.options.clone())
        };
        let target = Target {
            format: out.to.format.clone(),
            options,
            via: out.to.via.clone(),
        };
        let result = self.mediator.convert(source, target, cx).await?;

        let cannot_write = || {
            Error::msg(format!(
                "a `{}` value cannot be written to a file",
                result.format()
            ))
        };
        // The source's path inside its innermost container (or on the input).
        let inner = cx.mounts.path_in_container(from.trim_matches('/'));
        if let Some(bundle) = result.downcast_ref::<BundleData>() {
            // A composite output gets its own folder.
            let folder = match &out.path {
                Some(p) => p.trim_end_matches('/').to_owned(),
                None => without_extension(inner).to_owned(),
            };
            let mut files = Vec::new();
            for (name, item) in bundle.names.iter().zip(&bundle.items) {
                let blob = item.as_blob().ok_or_else(cannot_write)?;
                let path = format!("{folder}/{name}");
                cx.write_blob(&path, item.key(), blob).await?;
                files.push(path);
            }
            return Ok(files);
        }
        let blob = result.as_blob().ok_or_else(cannot_write)?;
        let path = match &out.path {
            Some(p) => p.clone(),
            // A copy of the source keeps its name.
            None if result.format().name() == crate::value::FILE => inner.to_owned(),
            None => {
                let name = out.to.format.name();
                with_extension(inner, self.mediator.extension_of(name).unwrap_or(name))
            }
        };
        cx.write_blob(&path, result.key(), blob).await?;
        Ok(vec![path])
    }
}

/// One conversion: a concrete source of an output request.
struct Job<'a> {
    from: String,
    format: Option<&'a FormatId>,
    out: &'a OutputRequest,
    /// The source could not be expanded (`NoMatch`, bad glob).
    error: Option<Error>,
}
