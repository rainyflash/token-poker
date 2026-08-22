#![no_main]

use libfuzzer_sys::fuzz_target;
use token_holdem_network::{decode_payload, HandPrivateMessage, HandPublicMessage};

const MAX_INPUT_BYTES: usize = 256 * 1_024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    let _ = decode_payload::<HandPublicMessage>(data);
    let _ = decode_payload::<HandPrivateMessage>(data);
});
