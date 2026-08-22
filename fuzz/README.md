# Protocol Fuzzing

The fuzz targets cover untrusted boundaries: control-wire CBOR, dynamic table-pool messages, table messages, private-room and recovery codes, and sidecar NDJSON commands.

`cargo-fuzz` requires nightly Rust, LLVM sanitizers, and a Unix-like environment. Windows can compile and unit-test the targets; run libFuzzer in Linux CI:

```bash
cargo +nightly fuzz run control_wire -- -max_total_time=60
cargo +nightly fuzz run table_pool -- -max_total_time=60
cargo +nightly fuzz run hand_messages -- -max_total_time=60
cargo +nightly fuzz run protocol_codes -- -max_total_time=60
cargo +nightly fuzz run ndjson_command -- -max_total_time=60
```

Fuzzing finds some crashes, bounds errors, and parser defects. It does not replace cryptographic or protocol security review.
