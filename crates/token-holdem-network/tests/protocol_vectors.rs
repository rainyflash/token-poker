#[path = "support/protocol_vectors.rs"]
mod protocol_vectors;

use protocol_vectors::{build_protocol_vectors, ProtocolVectorFile};

#[test]
fn 协议十一固定向量不得静默漂移() {
    let expected: ProtocolVectorFile =
        serde_json::from_str(include_str!("../../../test-vectors/protocol-11/core.json"))
            .expect("仓库中的协议固定向量必须是合法 JSON");
    let actual = build_protocol_vectors();

    assert_eq!(
        actual, expected,
        "协议编码或摘要发生变化；若这是有意的破坏性升级，请提高协议版本并运行生成脚本"
    );
}
