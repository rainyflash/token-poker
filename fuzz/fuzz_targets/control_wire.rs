#![no_main]

use libfuzzer_sys::fuzz_target;
use token_holdem_network::{decode_payload, ControlRequest, ControlResponse};

const MAX_INPUT_BYTES: usize = 64 * 1_024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let _ = decode_payload::<ControlRequest>(data);
    let _ = decode_payload::<ControlResponse>(data);
});
