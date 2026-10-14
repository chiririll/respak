# BCF1, RADA · Miles audio codecs (Bink Audio, RAD Audio)
**rsx sources:** `game/audio/miles_bcf.cpp` (Bink Audio ASI shim), `game/audio/miles_rada.cpp` (RAD Audio ASI shim), `game/audio/source.cpp` `DecodeAudioDataForSource` / `ExportAudioSourceAsset`, `game/audio/miles.h` (ASI table, ids), `game/audio/wavefile.h`, `thirdparty/radaudio/rada_{decode,file_header}.h`, `tools/prebuild.ps1` · **Games / versions:** Titanfall 2 and Apex Legends: BCF1 everywhere; RADA from Apex Season 24 on (`source.cpp:71-72`) · **Priority hint:** high as soon as `miles` (containers) exists, because nothing plays without it. BCF1: **M** (small Rust decoder, the algorithm is public). RADA: **L to XL** (no open decoder, see Decoding §RADA).

Container side (`.mbnk` banks, `.mstr` streams, `asrc`/`aevt`, how to find a source's header and data bytes): **[miles.md](miles.md)**. This file starts at "here is the encoded stream header and the encoded data of one source" and ends at PCM samples.

## Summary
Every Miles audio source (`asrc`) in a Respawn game is one encoded stream with one of two codecs:
- **BCF1 = Bink Audio** (RAD's Bink Audio, the same codec as audio in Bink videos and Unreal Engine's `.binka`), DCT variant, up to 2 channels per sub-stream, 16-bit PCM out of the RAD library.
- **RADA = RAD Audio** (RAD's newer MDCT codec, 2023, shipped with Unreal Engine 5.4+), up to 32 channels, 32-bit float out.

A user converts a source to WAV (rsx also offers Opus). In respak: a value with the `Audio` capability, converted by the existing `wav` edge.

**rsx has no decoder of its own for either codec.** It writes a Miles "ASI" shim around RAD's closed decoders that Epic ships for Unreal Engine, linked **statically** (not a DLL from the game install):
- Bink Audio: `binka_ue_decode_win64_static_x64{,D}.lib` (`UnrealBinkAudioDecodeInterface()`, `miles_bcf.cpp:44-45`; `rsx.vcxproj:622`).
- RAD Audio: `radaudio_decoder_win64.lib` (`miles_rada.cpp:8`, symbols wrapped with `RADA_WRAP=UERA`, `rsx.vcxproj:611`).
- Both libs are downloaded at build time from Epic's UE dependency CDN (`https://cdn.unrealengine.com/dependencies/...`) and checked by SHA-256 (`tools/prebuild.ps1:86-93`). They fall under the Unreal Engine EULA, not an open licence. rsx never loads `mss64.dll`, `binkawin64.dll` or any game DLL.

So the layouts below come from rsx's shim plus the public RAD headers rsx vendors. The BCF1 bitstream is documented from FFmpeg (and NihAV), which decode the same codec. The RADA bitstream is **unknown**.

## Where it lives
- Inside a Miles stream file (`.mstr`). Per source, the bank's source record (`MilesSource_v13/v28/v39/v48_t`, `source.h:15-134`) gives `streamHeaderOffset` + `streamHeaderSize` (the codec header, absolute offset in the `.mstr`) and `streamDataOffset` (the encoded blocks, relative to the `.mstr` header's `streamDataOffset`, `miles.h:65-75`). See [miles.md](miles.md).
- The codec stream is split in two places on disk. rsx glues it back together with `ReadAudioStream` (`source.cpp:19-43`): first `headerSize` bytes from `streamHeaderOffset`, then the data from `mstr.streamDataOffset + source.streamDataOffset`, for `fileSize - streamHeaderSize` bytes (`source.cpp:119-131`). **Header bytes followed by data bytes form a complete standalone codec file**, and that is the unit to hand a decoder.
- Codec id = first u32 of the stream header (`source.cpp:73-86`):

| rsx literal | u32 LE value | bytes on disk | codec | rsx decoder type (`miles.h:5-6`) |
|---|---|---|---|---|
| `'BCF1'` | `0x42434631` | `31 46 43 42` = `"1FCB"` | Bink Audio | `MILES_DECODER_BINKA = 2` |
| `'RADA'` | `0x52414441` | `41 44 41 52` = `"ADAR"` | RAD Audio | `MILES_DECODER_RADA = 6` |
| anything else | | | "MILES: Unsupported audio format." (`source.cpp:81-91`) | |

  MSVC multi-character literals are big-endian in the value, so the file bytes read reversed. `"1FCB"` is exactly the magic FFmpeg's `binka` demuxer probes for UE `.binka` files (`libavformat/binka.c`), and `"ADAR"` is the signature UE `.rada` files start with (vgmstream issue #1826). These are the stock RAD file formats, not Miles-specific variants.
- No name, no FourCC of its own, no guid: a codec stream is reached only through its `asrc`.

## Versions and detection
- **BCF1:** header `version` u8 at +4. rsx accepts `version <= 2` (`miles_bcf.cpp:54`, `:122`). v1 and v2 differ only in the u32 at +20 (v2: low 16 bits = seek entry count, high 16 bits = an extra field, `miles_bcf.cpp:57`, `:125-134`). FFmpeg's probe accepts versions 1 and 2. Version 0 is let through by rsx, but no one has seen it (open question).
- **Bink Audio 2 bitstream flag (BCF1):** rsx tells the UE decoder `is_bink_audio_2 = HIWORD(maxCompSpaceNeeded) != 0` (`miles_bcf.cpp:222-225`). This is a different bitstream (7-bit quantiser indices, see Decoding). It is a per-file flag, not the header version.
- **RADA:** `version` u8 at +8. The UE lib validates it inside `RadAGetFileHeader` (closed); rsx doesn't check it. Accepted versions are **unknown**.
- Proposed score (on the reassembled stream, or on the `asrc` typed entry handing it out): `"1FCB"` + version 1..=2 + channels 1..=16 + plausible rate → 10; `"ADAR"` → 10 (open → `UnsupportedVersion` until a RADA decoder exists).
- Proposed `Version` mapping: BCF1 `{ major: 1, minor: header.version }`, Bink Audio 2 flag kept as a decoder parameter, not a version. RADA `{ major: 2, minor: header.version }`. Unknown versions → `UnsupportedVersion`.

## Layout
Little-endian. Offsets from the start of the codec stream (= `streamHeaderOffset`).

### BCF1 file header (`BinkFileHeader_t`, `miles_bcf.cpp:6-17`, 24 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | u32 | `magic` | `0x42434631` (`"1FCB"` on disk) |
| 0x04 | 1 | u8 | `version` | 1 or 2 (rsx: `<= 2`) |
| 0x05 | 1 | u8 | `channelCount` | total channels. rsx sizes per-stream arrays for 8 streams = 16 ch (`miles_bcf.cpp:151-154`) but the decoder struct holds 4 stream pointers (`:106`), so >8 ch overflows in rsx |
| 0x06 | 2 | u16 | `sampleRate` | Hz, stored directly |
| 0x08 | 4 | u32 | `sampleCount` | frames **per channel** (rsx allocates `channels * sampleCount` values, `source.cpp:135`) |
| 0x0C | 4 | u32 | `maxCompSpaceNeeded` | low 16 bits: the largest frame payload in bytes (frames larger than this are rejected, `miles_bcf.cpp:284`, `:505`). High 16 bits: non-zero = Bink Audio 2 bitstream (`:223`) |
| 0x10 | 4 | u32 | `fileSize` | the whole codec stream in bytes (header + seek table + frames). rsx uses `fileSize - streamHeaderSize` as the number of data bytes to read (`miles_bcf.cpp:147` stores it at decoder +0x18, read back at `source.cpp:131`) |
| 0x14 | 4 | u32 | `seekTableEntryCount` | v1: whole u32 = entry count. v2: low u16 = entry count, high u16 = unknown, probably "blocks per seek table entry" (rsx's decoder struct names the slot `blocksPerSeekTableEntry_v2`, `:103`) |

Then the **seek table**: `entryCount` × u16, **deltas** in bytes (rsx: "Seek table is relative on disk", `miles_bcf.cpp:199-206`). Absolute entry `k` = sum of deltas `0..k`; one extra entry = sum of all (`:214`). What each entry points at (a frame, or every N-th frame, see the v2 high word) is unknown; a decoder that plays from the start doesn't need it. The header plus seek table is `24 + 2 * entryCount` bytes (`seekTableSize`, `:148`). FFmpeg skips exactly this much before the first frame (`binka.c`: `avio_skip(8); entries = rl16; skip(entries * 2 + 2)`).

Then **frames**, back to back until `fileSize`:
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| +0 | 2 | u16 | sync | `0x9999` (`miles_bcf.cpp:267`, `:415`, `:482`) |
| +2 | 2 | u16 | `size` | payload bytes, if `!= 0xFFFF` |
| +4 | 2 | u16 | `size` | only if the previous field is `0xFFFF`: payload bytes |
| +6 | 2 | u16 | `validSamples` | only in the 0xFFFF form: frames per channel to keep from this block (the end trim of the last block, `miles_bcf.cpp:343-354`) |
| +4 or +8 | `size` | bytes | payload | one Bink Audio block per sub-stream, see Decoding |

Frame validity as rsx checks it (`ASI_decode_block`, `miles_bcf.cpp:413-453`): sync matches; `size <= maxCompSpaceNeeded`; the frame ends inside the buffer; if 4 more bytes follow, they start with `0x9999`. On failure rsx resyncs by scanning byte by byte for `0x9999` with a valid size, or `"1FCB"` (a new header, skipped by `seekTableSize`) (`get_block_size_internal`, `:249-307`).

Constants derived from the header (`miles_bcf.cpp:59-86`):
- `maxSamplesPerFrame` = 2048 if rate ≥ 44100, 1024 if ≥ 22050, else 512 (= transform length; the samples actually emitted per block are 15/16 of it, see Decoding).
- sub-streams: `streamCount = (channels + 1) >> 1`. Stream `i` has 2 channels, the last one 1 if `channels` is odd (`:67-77`, `:158-172`).
- decode output format 0 = 16-bit PCM (`:86`).

### RADA file header (`RadAFileHeader`, `thirdparty/radaudio/rada_file_header.h:18-47`, 40 bytes)
Offsets computed from natural alignment (no `#pragma pack` in the header); verify against a real file.
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | u32 | `tag` | `0x52414441` (`"ADAR"` on disk) |
| 0x04 | 4 | u32 | `bytes_for_first_decode` | bytes needed to decode the first block |
| 0x08 | 1 | u8 | `version` | accepted values unknown |
| 0x09 | 1 | u8 | `channels` | ≤ `RADA_MAX_CHANNELS = 32` (`:7`) |
| 0x0A | 2 | u16 | `rada_header_bytes` | size of the per-stream encoder headers after the file (and seek) header |
| 0x0C | 1 | u8 | `shift_bits_for_seek_table_samples` | |
| 0x0D | 1 | u8 | `bits_for_seek_table_bytes` | |
| 0x0E | 1 | u8 | `bits_for_seek_table_samples` | |
| 0x0F | 1 | u8 enum | `sample_rate` | `ERadASampleRate`: 0 = 24000, 1 = 32000, 2 = 44100, 3 = 48000 (`:9-16`; rsx maps it at `miles_rada.cpp:28-43`, other values give rate 0) |
| 0x10 | 8 | u64 | `frame_count` | frames per channel. rsx rejects > u32 (`miles_rada.cpp:24-26`) |
| 0x18 | 8 | u64 | `file_size` | whole codec stream. rsx rejects > u32. rsx reads the low u32 at +0x18 of the opened container (the container starts with this header, `rada_decode.h:102`) as the stream size (`source.cpp:131`) |
| 0x20 | 2 | u16 | `seek_table_entry_count` | 0 = no seek table |
| 0x22 | 2 | u16 | `max_block_size` | largest encoded block incl. its block header |
| 0x24 | 4 | u16[2] | `PADDING` | |

Order on disk (`rada_decode.h:17-23`, `rada_file_header.h:71-75`): FileHeader (40) · `RadASeekTableHeader` (64 bytes: `i64 byte_line_slope[2], byte_intercept, byte_bias, sample_line_slope[2], sample_intercept, sample_bias`, `rada_file_header.h:49-57`) **only if** `seek_table_entry_count != 0` · per-stream encoder headers, `rada_header_bytes` in total (their layout and the stream count are **unknown**) · bit-packed seek table, `8 * ceil((bits_for_seek_table_samples + bits_for_seek_table_bytes) * entries / 64)` bytes (`rada_decode.h:59-67`) · encoded blocks.

Bytes needed to open (= offset of the first block, `RadAGetBytesToOpen`, `rada_decode.h:75-85`): `40 + rada_header_bytes`, plus `64 + seek table size` when there is a seek table.

Block layout: **unknown** (inside the closed lib). Known only: `RADA_SYNC_BYTE = 0x55` (`rada_file_header.h:6`, presumably the first byte of a block), at most `RadADecodeBlock_MaxOutputFrames = 1024` frames per block (`rada_decode.h:214`), and the first block after a start or seek only primes the MDCT and may emit 0 frames (`:189-191`, `:228`).

## Data outside the header
- Everything is in the `.mstr` (see [miles.md](miles.md)); the header and the data are separate ranges of the same file. No extra compression around the codec stream.
- Sizes for a ranged read: BCF1 needs `24 + 2*entries` header bytes, then `fileSize - streamHeaderSize` data bytes. RADA: `RadAGetBytesToOpen` header bytes, then `file_size - streamHeaderSize` data bytes.
- Whether `streamHeaderSize` always equals `24 + 2*entries` (BCF1) or `RadAGetBytesToOpen` (RADA) is unverified. rsx doesn't check it (open question).

## Dependencies
None. The codec stream is self-contained; the `asrc` that points at it is covered in [miles.md](miles.md).

## Decoding
### What rsx does (both codecs), `DecodeAudioDataForSource`, `source.cpp:57-286`
1. Read the stream header (`streamHeaderSize` bytes), pick the decoder by magic (`:64-92`).
2. `ASI_stream_parse_metadata` → channels, rate, frame count and `{minSizeToOpen, maxBlockSize, maxSamplesPerDecode, decodeFormat}` (`:96-113`).
3. `ASI_open_stream` reads the header again through `ReadAudioStream` (`:115-127`); `ASI_notify_seek` resets the overlap state (`:129`).
4. Loop until `channels × frameCount` values are written (`:145-270`): ask `ASI_get_block_size` how much to read, read it, `ASI_decode_block`, keep any unconsumed bytes.
5. Every decoder output is **planar**: channel 0's samples, then channel 1's, … For BCF1 the per-channel stride is the number of samples decoded; for RADA it is `maxSamplesPerDecode` = 1024, padded (`:216-221`). rsx interleaves into one buffer and **clamps the last block** to `frameCount` (`:224`), so trailing padding is dropped.

Output buffer: interleaved i16 (BCF1, `decodeFormat 0`) or interleaved f32 (RADA, `decodeFormat 2`, `miles_rada.cpp:52`). Channel order as stored, never remapped for WAV.

### BCF1 (Bink Audio, DCT), enough to write the decoder
rsx calls `DecodeFn` of RAD's lib once per sub-stream on one shared input pointer (`miles_bcf.cpp:363-375`). The bitstream inside the payload is documented in FFmpeg `libavcodec/binkaudio.c` (LGPL-2.1+, decoder `binkaudio_dct`) and NihAV `nihav-rad/src/codecs/binkaud.rs` (AGPL-3.0, the user's fork `chiririll/nihav`). The pseudocode below follows FFmpeg; NihAV differences are noted.

Setup per file (FFmpeg `decode_init`; rsx's `maxSamplesPerFrame` agrees):
```
frame_len_bits = rate < 22050 ? 9 : rate < 44100 ? 10 : 11
frame_len   = 1 << frame_len_bits         // 512 / 1024 / 2048
overlap_len = frame_len / 16              // 32 / 64 / 128
emit        = frame_len - overlap_len     // samples per channel per block: 480 / 960 / 1920
root        = frame_len / (sqrt(frame_len) * 32768)          // DCT variant
quant_table[i] = exp(i * 0.15289164787221953) * root,  i = 0..95   // 0.0664 / log10(e)
half        = (rate + 1) / 2              // NihAV uses rate >> 1: differs for odd rates only
CRIT = [100,200,300,400,510,630,770,920,1080,1270,1480,1720,2000,2320,2700,3150,
        3700,4400,5300,6400,7700,9500,12000,15500,24500]          // ff_wma_critical_freqs
num_bands = 1; while num_bands < 25 && half > CRIT[num_bands-1]: num_bands++
bands[0] = 2; bands[i] = (CRIT[i-1] * frame_len / half) & ~1  (i = 1..num_bands-1); bands[num_bands] = frame_len
transform: inverse DCT (DCT-III) of length frame_len, scale 1/frame_len in FFmpeg's av_tx terms
v2 = HIWORD(maxCompSpaceNeeded) != 0          // rsx's is_bink_audio_2; FFmpeg: extradata "2" (UEBA files)
first = true; previous[ch][overlap_len] = 0
```
Per frame (after the 4- or 8-byte frame header): one bit reader, LSB-first ("LE" bit order, as in FFmpeg's `get_bits` on Bink audio and NihAV's `BitReaderMode::LE`), over the payload.
```
for each sub-stream s (2 channels, last one 1 if odd):
    skip 2 bits
    for each channel c of s:
        c0 = read_float(); c1 = read_float()              // 5-bit exp, 23-bit mantissa, 1 sign: ldexp(mant, exp-23), negate if sign
        coeffs[0] = c0 * root; coeffs[1] = c1 * root
        qbits = v2 ? 7 : 8
        quant[b] = quant_table[min(read(qbits), 95)]  for b in 0..num_bands
        i = 2; k = 0; q = quant[0]
        while i < frame_len:
            if read(1): j = i + RLE[read(4)] * 8          // RLE = [2,3,4,5,6,8,9,10,11,12,13,14,15,16,32,64]
            else:       j = i + 8
            j = min(j, frame_len)
            width = read(4)
            if width == 0:
                coeffs[i..j] = 0; i = j
                while bands[k] < i: q = quant[k++]
            elif !v2:
                while i < j:
                    if bands[k] == i: q = quant[k++]
                    m = read(width)
                    coeffs[i] = m == 0 ? 0 : (read(1) ? -q*m : q*m); i++
            else:                                          // Bink Audio 2: all magnitudes first, then signs
                m[i..j] = read(width) each
                while i < j:
                    if bands[k] == i: q = quant[k++]
                    coeffs[i] = m[i] > 0 ? (read(1) ? -q*m[i] : q*m[i]) : 0; i++
        coeffs[0] *= 2                                     // FFmpeg "coeffs[0] /= 0.5"
        out[c][0..frame_len] = IDCT(coeffs)
    // overlap with the previous block (FFmpeg form; n = channels in this sub-stream)
    for each channel c of s:
        if !first:
            count = overlap_len * n
            for i in 0..overlap_len: jj = c + i*n
                out[c][i] = (previous[c][i] * (count - jj) + out[c][i] * jj) / count
        previous[c] = out[c][frame_len - overlap_len .. frame_len]
    first = false   // FFmpeg clears it inside decode_block, i.e. after the first sub-stream: check with >2 ch
    align the bit reader to 32 bits (relative to the payload start)
emit out[c][0..emit] for every channel
if the frame had the 0xFFFF form: keep only the first validSamples samples per channel
```
rsx's trim (`miles_bcf.cpp:378-393`): the planar buffer holds `decoded/channels` samples per channel; channel `k`'s first `validSamples` samples are moved to `k*validSamples`, and the block counts as `validSamples` frames. FFmpeg's UEBA demuxer does the same as "discard padding" from the end (`binka.c`: `frame_samples - duration`).

Output of RAD's decoder is 16-bit PCM (`miles_bcf.cpp:30`, `:86`). FFmpeg/NihAV produce float in about `-1..1`. respak's `AudioChunk` is f32, so a Rust decoder emits floats directly. The 16-bit rounding of the RAD lib is lost, which is expected; compare with a tolerance.

NihAV vs FFmpeg, for whoever reuses NihAV's `binkaud.rs`:
- NihAV: ≤ 2 channels only (`validate!(channels <= 2)`), no Bink Audio 2 (7-bit quantisers, magnitudes-then-signs), a packet must start with a u32 "decoded byte count" (Bink video packets have it, BCF1 frames don't), linear overlap weights `(len-i)/len` (FFmpeg interleaves the weights by channel). The fork's commit `895ba0d` fixed the DCT DC/scale. All differences are small, but **multi-stream (>2 ch) and v2 are missing**.
- FFmpeg handles >2 channels as consecutive ≤2-channel blocks in one packet (`ch_offset`, up to 6 channels for DCT), the same structure as rsx's sub-streams.

Unknown for BCF1: whether any Miles file uses the RDFT variant (FFmpeg treats every `1FCB`/`UEBA` as DCT; rsx passes no variant flag to the UE lib, so the lib only does one). Assume DCT.

### RADA (RAD Audio)
**rsx delegates entirely to the closed UE library**; nothing of the bitstream is known. Call interface, so a future decoder can mirror the flow (`miles_rada.cpp`, `rada_decode.h`):
1. `RadAGetFileHeader(buf, len)` validates the header (`miles_rada.cpp:14`). `RadAGetMemoryNeededToOpen(buf, len, &n)` gives the container size (0 = ok, -1 = bad data, >0 = needs more bytes; rsx treats >0 as failure, `:21-22`).
2. Read `RadAGetBytesToOpen(header)` bytes (header + seek header + stream headers + seek table), `RadAOpenDecoder(data, len, container, containerSize)` (`:60-103`). Returns ASI 2 if there is a seek table, else 1.
3. `RadANotifySeek(container)` before the first block (`source.cpp:129`).
4. Loop: `RadAExamineBlock(container, buf, len, &needed)` → `Valid` / `Incomplete` (read more, `needed` bytes) / `Invalid` (`miles_rada.cpp:213-230`). Then `RadADecodeBlock(container, buf, len, float_out, stride_in_floats, &consumed)` → frames emitted (0 is normal at the start), `-1` done, `-2` error (`:165-211`). The output is **planar f32**, channel `c` at `out[c * stride]`; rsx's stride = 1024.
5. rsx reads 9 extra bytes beyond the reported block size (`*a6 = 9`, `miles_rada.cpp:228`, `source.cpp:182`); why 9 is unknown (probably the block header size).

Options for respak (the user doesn't want RAD binaries, C, or DLLs):
| Option | Licence fit | Effort | Note |
|---|---|---|---|
| a. Link/load Epic's `radaudio_decoder` lib (what rsx does) | UE EULA: **not** AGPL-compatible, binary blob | S | Rejected by the project rules |
| b. Port UE's RAD Audio source (it is in the UE source tree under the UE EULA) | **Not allowed**: EULA code can't go into an AGPL project; reading it taints a clean-room port | M | Rejected |
| c. Clean-room reverse engineering of the decoder from binaries plus black-box tests (the `oozextract` precedent, PROJECT.md §3) | Own code, AGPL | L–XL | The only open route; needs a spec written by someone else before a separate implementation |
| d. Wait for a third-party open decoder (vgmstream wants proper RE for Bink Audio and has no RADA; GhostScissors' `RadA-Decoder` exists, licence and implementation unknown, probably a wrapper of the UE lib) | depends | 0 now | Re-check later |
| e. Support RADA metadata only: header, duration, channels; decoding gives `UnsupportedVersion` | fine | S | **Recommended for now**, with (c) or (d) as a separate later decision |

## Output
rsx (`ExportAudioSourceAsset`, `source.cpp:399-570`):
- **WAV** (`source.cpp:448-471`, `wavefile.h`): 44-byte `RIFF`/`WAVE`/`fmt `(16)/`data` header, written after the data and rewritten at offset 0. `formatTag` = 3 (IEEE float) for RADA, 1 (PCM) for BCF1; `channels`; `sampleRate`; `blockAlign = dataSize / sampleCount`; `bitsPerSample = dataSize*8 / sampleCount / channels` (16 or 32); `avgBytesPerSecond = blockAlign * rate`; `RIFF size = dataSize + 36`. No `WAVE_FORMAT_EXTENSIBLE`, no channel mask even for 6 or 8 channels, no `fact` chunk for float. The data is interleaved in the decoder's channel order.
- **Opus** (`source.cpp:472-567`, libopusenc): for 3..8 channels, the channels are reordered from WAV order to Vorbis order with `{3:[0,2,1], 5:[0,2,1,3,4], 6:[0,2,1,4,5,3], 7:[0,2,1,5,6,4,3], 8:[0,2,1,6,7,4,5,3]}` (`:475-516`), mapping family 1, fixed bitrates 160–448 kbit/s (`:543-556`), tags TITLE/COMMENT/COPYRIGHT/GAME. These tables imply that **decoded order = Microsoft WAV order** (FL FR FC LFE BL BR SL SR); unverified.
- Path: `<export>/audio/<name>.wav` or the full asset path (`:407-422`), i.e. global settings.

Proposed respak:
- The source value (from [miles.md](miles.md)) implements **`Audio`** (`respak-core/src/capability/audio.rs`): one track, `AudioTrackInfo { channels, sample_rate }` from the codec header, interleaved f32 `AudioChunk`s, one per decoded block (or a few), trimmed to `frameCount`. RADA: `tracks()` still reports the shape; `samples()` → `UnsupportedVersion` until a decoder exists.
- WAV through the existing `wav` edge (`respak-formats/src/wav.rs`, always 16-bit PCM). For BCF1 this loses nothing: the RAD lib itself outputs 16-bit, and `pcm16(x/32768)` round-trips. A float WAV option (for RADA) and `WAVE_FORMAT_EXTENSIBLE` with a channel mask for >2 channels are possible later `wav` options. Opus is out of scope (C library).

## rsx quirks
- **BCF1 seek table decode is broken** (`miles_bcf.cpp:179-206`): u16 entries are read into `char seekTableBuffer[256]` but summed per byte, as signed chars (`lastSeek += seekTableBuffer[seekIdx]`), so absolute offsets are garbage. Harmless in rsx because it always decodes from the start; don't port, decode deltas as u16.
- The size check differs: `get_block_size` uses `LOWORD(maxCompSpaceNeeded)` in the resync path (`:284`) but the full u32 elsewhere (`:439`, `:505`). With the Bink Audio 2 flag set in the high word the full compare is looser. Use the low word.
- `channelsPerStream`/`decoderSizePerStream` hold 8 streams, `streamDecoders` holds 4 (`:106`, `:151-154`): >8 channels corrupts memory. respak: reject channel counts it can't verify instead of writing out of bounds.
- The BINKA read loop resizes the input to 8 and reads over the unconsumed tail (`source.cpp:154-161`, `:264-265`). It works only because each decode consumes the whole frame.
- `decoder->blocksPerSeekTableEntry_v2` is overwritten with `fileSize` so the generic loop can read the stream size at +0x18 for both codecs (`miles_bcf.cpp:147`, `source.cpp:131`). Note the v2 high word of `seekTableEntryCount` is extracted (`unk`) and then dropped.
- RADA `ASI_open_stream` leaks the header buffer when there is no seek table (`miles_rada.cpp:96-102`) and on `RadAGetFileHeader` failure (`:67-70`).
- RADA sample rate enum values ≥ 4 give a rate of 0 (`miles_rada.cpp:28-43`), and a WAV with rate 0. respak: `Corrupt`.
- Header errors become `return false` / `0`; the export logs and returns `false` (`source.cpp:88-91`, `:434-438`). Results of `ASI_open_stream` are ignored (`:127`).
- The whole source is decoded into one buffer before writing (`source.cpp:135-136`); respak streams chunks.
- WAV: `blockAlign` and `bitsPerSample` are recomputed by division from the data size, `long` fields (4 bytes on MSVC); a `sampleCount` of 0 divides by zero (`source.cpp:461-462`).
- `MILES_NO_RADAUDIO` builds stub out RADA entirely (`miles_rada.cpp:253-260`).

## Reuse in respak
- `respak-core` `Audio` capability and `respak-formats` `wav` edge: as is.
- NihAV (`chiririll/nihav` fork, AGPL-3.0, already a git dependency of `respak-nihav`; `crates/respak-nihav/src/bik_decode.rs:153-209` wraps its audio decoder): `binkaud.rs` + `binkauddata.rs` (bands, quant table, bit reader, DCT) are the closest existing code. Two ways:
  1. **Recommended:** extend the fork's `BinkAudioDecoder` (it is the user's fork) with: Bink Audio 2 (7-bit quantisers, magnitudes-then-signs), more than 2 channels as consecutive ≤2-channel sub-blocks sharing one bit reader with 32-bit alignment between them, and a raw-block entry point without the u32 size prefix. Then a small `bcf1` reader in respak (frame parsing, trimming, planar → interleaved) feeds it. Keeps one Bink Audio implementation for `bik` and Miles.
  2. A standalone Rust decoder in respak written from the pseudocode above (needs a DCT-III; `nihav_codec_support::dsp::dct` is already in the dependency tree). FFmpeg's code is LGPL-2.1-or-later, which may be used under GPL-2.0+ terms and therefore with AGPL-3.0 (via GPLv3 §13); keep its copyright notice if code is ported, or write from the spec.
- Home: the codec next to the other RAD code in `respak-nihav` (same reasoning as task 15, decision 1). The `asrc` value lives wherever `miles` goes and asks the codec for an `Audio`.
- New: frame parser and `0xFFFF` trim handling, Bink Audio 2 bits, multi-stream, RADA header parse (metadata only).

## Tests
Synthetic (no game files):
- **BCF1 header:** 24 bytes `31 46 43 42 | 02 | 02 | 80 BB | <count> | <maxComp> | <fileSize> | <entries>` parses (48000 Hz → frame_len 2048, emit 1920; 22050 → 1024/960; 16000 → 512/480); version 3 → `UnsupportedVersion`; wrong magic → not scored; v2 high word ignored for the entry count; `maxComp` high word set → v2 bitstream chosen.
- **Seek table:** deltas `[10, 300, 65535]` → absolute `[0, 10, 310, 65845]` (catches rsx's signed-char bug).
- **Frame parser:** `99 99 <size>`; `99 99 FF FF <size> <valid>`; size > `maxComp` → `Corrupt`; missing sync after a frame → `Corrupt` (or resync, decide); truncated last frame yields earlier chunks then one `Err`.
- **Silent block:** a hand-packed payload with `c0 = c1 = 0`, all quantisers 0, every run `width = 0` decodes to `emit` zero samples per channel, for 1, 2, 3 and 6 channels (3 = one stereo + one mono sub-stream; 32-bit alignment between them).
- **Single DC coefficient:** a block with only `coeffs[0]` set gives a constant (after the DCT) of a value computed by hand from `root`; second block checks the overlap ramp.
- **Trim:** the last frame in 0xFFFF form with `valid = 100` → exactly 100 samples per channel; total frames == header `sampleCount`.
- **RADA header:** 40 bytes with `sample_rate = 3` → 48000; `= 4` → `Corrupt`; `RadAGetBytesToOpen` arithmetic with and without a seek table; `samples()` → `UnsupportedVersion`.
- No panics on random bytes after a valid header.

Real files (local only, via env var, never committed; no Steam runs without permission):
- Reassemble a few BCF1 streams (header bytes ++ data bytes, see Where it lives) from Titanfall 2 and pre-S24 Apex `.mstr`: mono, stereo, a >2-channel one if any exists, one with the v2 flag, one with the v1 header.
- **Oracle 1, FFmpeg:** a reassembled BCF1 file is a valid `.binka` for `ffmpeg -f binka -i x.binka -f f32le` **if** it uses the old bitstream and no 0xFFFF frames. For v2-flag files, rewrite the header as UEBA (bytes `"ABEU"`, `01`, channels, 2 pad bytes, rate u32, sample count u32, 8 bytes, entries u16, then 2 bytes and the seek table, per `binka.c`), which also makes FFmpeg honour the 0xFFFF trim. Expect max difference ≤ a few LSB at 16-bit.
- **Oracle 2, rsx:** the user exports the same sources to WAV with rsx (16-bit PCM for BCF1). The respak WAV should match sample for sample or within ±1–2 LSB (float vs RAD's integer path), same length, same channel order.
- RADA (S24+ Apex): header fields plausible (rate, channels, frame count × rate ≈ duration from the `asrc`), `samples()` gives `UnsupportedVersion`. Keep rsx float WAVs as the oracle for a future decoder.

## Open questions
1. **RADA route: decided (user, 2026-10-14): header only for now.** Parse the RADA header (channels, rate, frame count, size); `tracks()` reports the shape, `samples()` → `UnsupportedVersion`. A real decoder is a later decision (clean-room RE or an open decoder). Original question: accept metadata-only RADA now (recommended), and decide separately whether to fund a clean-room reverse engineering (L–XL) or wait for an open decoder? From Season 24 new Apex audio is RADA (`source.cpp:71-72`), so this decides how much of current Apex audio respak can play.
   Open-source check (2026-10-14): **no open decoder exists.** NihAV (incl. the `chiririll/nihav` fork) has only Bink video, Bink audio and Smacker under `nihav-rad`. FFmpeg has none. vgmstream issue #1826 was closed by its requester with a pointer to an external tool, not implemented. GhostScissors' `RadA-Decoder` (C++, no licence file) has no decoder of its own: its `.vcxproj` compiles `SDK/Src/RadAudio/radaudio_decoder*.c`, `radaudio_mdct.cpp`, … which are not in the repo; they are RAD Audio's C source shipped inside Unreal Engine (`Engine/Source/Runtime/RadAudioCodec/SDK`) under the UE EULA. So the decoder source is *readable* by UE licensees but not usable here: porting it would make respak a derivative of EULA code (not AGPL-compatible). A clean-room reverse engineering must not look at that source.
2. **BCF1 decoder home:** extend the `chiririll/nihav` fork's `binkaud.rs` (v2 bitstream, >2 channels, raw-block entry) or write a standalone decoder in respak?
3. **Bink Audio 2 flag:** rsx uses `HIWORD(maxCompSpaceNeeded) != 0` (`miles_bcf.cpp:223`). Confirmed only by rsx's reverse engineering, not by a spec; do real Apex files set it, and do v2-flag files decode correctly with FFmpeg's `version_2` path?
4. **v2 header high word** of `seekTableEntryCount`: "blocks per seek table entry"? Unknown and unneeded for linear decoding.
5. Does any Miles BCF1 file use the RDFT variant or more than 2 channels? What channel counts occur (rsx's Opus tables cover up to 8)?
6. ~~**Channel order** / `WAVE_FORMAT_EXTENSIBLE`~~ **decided (user, 2026-10-14): for now, export one mono WAV per channel** (a composite output, like `png.frames`), so no channel mask or speaker order is needed. Multichannel WAV with a channel mask may come later. Still unknown: which speaker each decoded channel is (matters only for that later option).
7. **Overlap weights:** FFmpeg interleaves the crossfade weights by channel, NihAV doesn't; which matches RAD's lib (and rsx's WAVs) better?
8. `streamHeaderSize` vs `24 + 2*entries` (BCF1) / `RadAGetBytesToOpen` (RADA): always equal? If larger, what fills the gap?
9. RADA: accepted `version` values, per-stream encoder header layout, block layout (sync `0x55`?), and why rsx reads 9 extra bytes: all unknown.
10. BCF1 header `version 0`: allowed by rsx, never seen. Reject?
11. Should respak offer float WAV (for RADA later) and Opus? Opus needs a C library or a pure-Rust encoder; probably not.
