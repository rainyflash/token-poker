#[path = "support/protocol_vectors.rs"]
mod protocol_vectors;

use protocol_vectors::{build_protocol_vectors, ProtocolVectorFile};

#[test]
fn 协议十二固定向量不得静默漂移() {
    let expected: ProtocolVectorFile =
        serde_json::from_str(include_str!("../../../test-vectors/protocol-12/core.json"))
            .expect("仓库中的协议固定向量必须是合法 JSON");
    let actual = build_protocol_vectors();

    assert_eq!(
        actual, expected,
        "协议编码或摘要发生变化；若这是有意的破坏性升级，请提高协议版本并运行生成脚本"
    );
}

#[test]
fn 牌规升级不改变已有动作与归档回执的编码() {
    let previous: ProtocolVectorFile =
        serde_json::from_str(include_str!("../../../test-vectors/protocol-11/core.json")).unwrap();
    let current = build_protocol_vectors();
    for name in ["signed-hand-action-v2", "co-signed-hand-receipt-v1"] {
        let expected = previous
            .vectors
            .iter()
            .find(|vector| vector.name == name)
            .unwrap();
        let actual = current
            .vectors
            .iter()
            .find(|vector| vector.name == name)
            .unwrap();
        assert_eq!(actual, expected);
    }
    assert_eq!(
        token_holdem_network::CONTROL_PROTOCOL,
        "/token-holdem/control/11"
    );
    assert_eq!(
        current.protocol_version.to_string(),
        token_holdem_network::PROTOCOL_VERSION
    );
}
