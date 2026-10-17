//! The plugin cache (slice 13): namespaced by plugin id and `cache_version`.

use respak_core::testing::*;
use respak_core::{Cx, Event, Plugin, Registry};

struct Versioned(&'static str, u32);

impl Plugin for Versioned {
    fn id(&self) -> &str {
        self.0
    }
    fn cache_version(&self) -> u32 {
        self.1
    }
    fn register(&self, _: &mut Registry) {}
}

fn cx_with_cache(plugins: &[&dyn Plugin], cache: Option<opendal::Operator>) -> Cx {
    let mut r = Registry::new();
    for p in plugins {
        r.add(*p);
    }
    let builder = CxBuilder::new(r, mem(), mem());
    match cache {
        Some(op) => builder.cache(op),
        None => builder,
    }
    .build()
}

#[tokio::test]
async fn no_cache_operator_means_no_cache() {
    let cx = cx_with_cache(&[&Versioned("a", 0)], None);
    assert!(cx.cache("a").is_none());
}

#[tokio::test]
async fn values_round_trip_and_can_be_removed() {
    let cx = cx_with_cache(&[&Versioned("a", 0)], Some(mem()));
    let cache = cx.cache("a").unwrap();
    assert_eq!(cache.get("k").await, None);
    cache.put("k", b"v".to_vec().into()).await;
    assert_eq!(cache.get("k").await.unwrap().as_ref(), b"v");
    cache.remove("k").await;
    assert_eq!(cache.get("k").await, None);

    cache.put_json("j", &serde_json::json!({"x": [1, 2]})).await;
    assert_eq!(
        cache.get_json::<serde_json::Value>("j").await,
        Some(serde_json::json!({"x": [1, 2]}))
    );
}

#[tokio::test]
async fn plugins_and_versions_do_not_see_each_other() {
    let op = mem();
    let v0 = cx_with_cache(&[&Versioned("a", 0), &Versioned("b", 0)], Some(op.clone()));
    v0.cache("a").unwrap().put("k", b"a0".to_vec().into()).await;
    assert_eq!(v0.cache("b").unwrap().get("k").await, None);

    let v1 = cx_with_cache(&[&Versioned("a", 1)], Some(op.clone()));
    assert_eq!(
        v1.cache("a").unwrap().get("k").await,
        None,
        "a new cache_version starts empty"
    );
    let again = cx_with_cache(&[&Versioned("a", 0)], Some(op));
    assert_eq!(
        again.cache("a").unwrap().get("k").await.unwrap().as_ref(),
        b"a0"
    );
}

#[tokio::test]
async fn unreadable_entries_are_a_miss_and_a_warning() {
    let cx = cx_with_cache(&[&Versioned("a", 0)], Some(mem()));
    let mut rx = cx.mediator.subscribe();
    let cache = cx.cache("a").unwrap();
    cache.put("bad", b"not json".to_vec().into()).await;
    assert_eq!(cache.get_json::<serde_json::Value>("bad").await, None);
    assert!(matches!(rx.try_recv(), Ok(Event::Warning { .. })));
}
