#![no_main]

use libfuzzer_sys::fuzz_target;
use token_holdem_identity::RecoveryEnvelope;
use token_holdem_network::{decode_code, FriendRoomInvite};

const MAX_CODE_BYTES: usize = 16 * 1_024;

fuzz_target!(|data: &[u8]| {
    let Ok(code) = std::str::from_utf8(data) else {
        return;
    };
    let _ = decode_code::<FriendRoomInvite>("TH1-", code, MAX_CODE_BYTES);
    let _ = decode_code::<RecoveryEnvelope>("THR1-", code, MAX_CODE_BYTES);
});
