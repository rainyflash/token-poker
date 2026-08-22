#![no_main]

use libfuzzer_sys::fuzz_target;
use token_holdem_sidecar::decode_command_line;

fuzz_target!(|data: &[u8]| {
    let Ok(line) = std::str::from_utf8(data) else {
        return;
    };
    let _ = decode_command_line(line);
});
