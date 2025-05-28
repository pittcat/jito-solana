// 文件功能说明：
// 这个文件专门用于解析 Solana 上的 Nonce（Number used once，只使用一次的数字）账户数据。
// Nonce 账户在 Solana 中是一种特殊用途的账户，主要用于处理交易的“持久性”和“幂等性”。
//
// 1. 持久性 (Durable Transactions):
//    通常，Solana 交易依赖于一个最近的区块哈希 (blockhash) 来进行验证，这个区块哈希是有时效性的（大约2分钟）。
//    如果一个交易因为网络延迟等原因，在区块哈希过期后才被节点处理，交易就会失败。
//    Nonce 账户可以为交易提供一个“持久的”区块哈希。一个 Nonce 账户会存储一个特定的区块哈希，
//    只要这个 Nonce 账户存在且其状态（即它存储的区块哈希）未被消耗，引用该 Nonce 账户的交易就可以在任何时候被处理，
//    即使全局的区块哈希已经更新。这对于需要延迟执行或确保在特定条件下最终能执行的交易非常有用。
//
// 2. 幂等性 (Idempotency):
//    幂等性意味着一个操作执行一次和执行多次的效果是相同的。
//    当一个交易使用 Nonce 账户成功执行后，Nonce 账户内部存储的区块哈希会发生改变（通常是更新为一个新的区块哈希）。
//    如果原始交易（或其副本）因为网络问题等被重复提交，由于它引用的 Nonce 账户状态已经改变，
//    这个重复的交易就会因为 Nonce 校验失败而无法再次成功执行，从而保证了操作的幂等性。
//
// Nonce 账户通常包含以下关键信息：
// - Authority (管理者): 有权管理这个 Nonce 账户的公钥。例如，管理者可以授权一个新的区块哈希给这个 Nonce 账户。
// - Blockhash (区块哈希): Nonce 账户当前存储的区块哈希，用于授权交易。
// - FeeCalculator (费用计算器): 与该区块哈希关联的交易费用计算规则。
//
// 本文件中的代码会将 Nonce 账户的原始二进制数据反序列化，并转换为用户友好的结构化信息，
// 方便钱包、浏览器等工具展示 Nonce 账户的状态和内容。

use {
    // 从当前 crate (solana-account-decoder) 导入解析错误类型和UI友好的费用计算器结构。
    crate::{parse_account_data::ParseAccountError, UiFeeCalculator},
    // 从 Solana 指令错误库导入错误类型，用于处理无效账户数据等情况。
    solana_instruction::error::InstructionError,
    // 从 Solana Nonce 程序接口库导入状态定义。
    solana_nonce::{
        state::State as NonceState, // `NonceState` 是链上 Nonce 账户的原始状态枚举 (Uninitialized, Initialized)。
                                    // 使用 `as NonceState` 是为了避免与可能存在的其他 `State` 类型冲突。
        versions::Versions as NonceVersions, // `NonceVersions` 用于处理 Nonce 账户状态的版本控制，确保兼容性。
    },
};

// 函数 `parse_nonce`：
// 功能：解析 Nonce 账户的原始二进制数据。
// 参数：
//   - `data`: `&[u8]`，一个字节切片，代表 Nonce 账户的原始数据。
// 返回值：
//   - `Result<UiNonceState, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(UiNonceState)`，其中 `UiNonceState` 是一个枚举，
//       表示 Nonce 账户的状态（目前主要是 `Initialized`，并包含其详细数据）。
//     - 如果解析失败（例如数据格式不正确或账户未初始化），返回 `Err(ParseAccountError)`。
// 设计思路：
//   1. 首先，尝试使用 `bincode::deserialize(data)` 将传入的原始字节数据 `data` 反序列化为 `NonceVersions` 结构。
//      `NonceVersions` 是一个包装器，它内部包含了实际的 `NonceState`，并处理版本兼容问题。
//      如果反序列化失败，说明数据不是一个有效的 Nonce 账户数据，此时映射错误为
//      `ParseAccountError::from(InstructionError::InvalidAccountData)` 并返回。
//      `.map_err(|_| ...)` 用于在 `Err` 的情况下转换错误类型。`_` 忽略原始的 bincode 错误。
//   2. 如果反序列化成功，我们得到了 `nonce_versions`。接着，调用 `nonce_versions.state()` 获取实际的 `NonceState`。
//   3. 使用 `match` 表达式根据 `NonceState` 的不同成员进行处理：
//      - `NonceState::Uninitialized`:
//        根据 Solana 的设计，一个账户如果仅仅是被分配了空间但没有写入任何 Nonce 初始化数据，
//        或者其数据长度不符合 Nonce 账户的要求，它不应该被成功解析为一个“未初始化的 Nonce 账户”。
//        这样的账户可能是系统账户，或者预留给其他程序使用。
//        因此，如果状态是 `Uninitialized`，此函数也返回 `InstructionError::InvalidAccountData` 错误，
//        表示这不是一个有效的、可供此解析器识别的 Nonce 账户状态。
//        （注意：虽然 `UiNonceState` 枚举中有一个 `Uninitialized` 成员，但此解析函数当前逻辑下不会主动返回它，
//         而是将这种情况视为解析错误。这可能是为了更严格地区分真正被初始化为 Nonce 账户但数据无效的情况，
//         与根本就不是 Nonce 账户或尚未被正确初始化的空账户的情况。）
//      - `NonceState::Initialized(data)`:
//        - 这表示 Nonce 账户已经成功初始化，并包含了有效的数据（在 `data` 变量中，类型为 `solana_nonce::state::Data`）。
//        - 我们从 `data` 中提取 `authority` (管理者地址), `blockhash` (区块哈希), 和 `fee_calculator` (费用计算器)。
//        - 将这些原始数据转换为用户友好的字符串或 `UiFeeCalculator` 格式。
//          - `data.authority.to_string()`: 将管理者公钥转换为字符串。
//          - `data.blockhash().to_string()`: 获取并转换区块哈希为字符串。
//          - `data.fee_calculator.into()`: 将原始的 `FeeCalculator` 转换为 `UiFeeCalculator` (因为 `UiFeeCalculator` 实现了 `From<FeeCalculator>`)。
//        - 构建一个 `UiNonceData` 结构体来存储这些转换后的信息。
//        - 最后，将构造好的 `UiNonceData` 包装在 `UiNonceState::Initialized` 中，并作为 `Ok` 结果返回。
pub fn parse_nonce(data: &[u8]) -> Result<UiNonceState, ParseAccountError> {
    // 1. 尝试将原始数据反序列化为 NonceVersions 结构。
    // NonceVersions 内部会处理版本信息并包含实际的 NonceState。
    let nonce_versions: NonceVersions = bincode::deserialize(data)
        .map_err(|_| ParseAccountError::from(InstructionError::InvalidAccountData))?; // 如果反序列化失败，视为无效账户数据。

    // 2. 获取实际的 NonceState 并进行匹配。
    match nonce_versions.state() {
        // 源码中的注释解释了为什么 Uninitialized 状态在这里被视为错误：
        // "This prevents parsing an allocated System-owned account with empty data of any non-zero
        // length as `uninitialized` nonce. An empty account of the wrong length can never be
        // initialized as a nonce account, and an empty account of the correct length may not be an
        // uninitialized nonce account, since it can be assigned to another program."
        // 简而言之，一个仅分配了空间但未正确初始化的账户，或者长度不符的账户，不应被视为有效的“未初始化Nonce账户”。
        // 它可能是系统账户或预留给其他用途。因此，直接返回错误。
        NonceState::Uninitialized => Err(ParseAccountError::from(
            InstructionError::InvalidAccountData,
        )),
        // 状态：已初始化
        NonceState::Initialized(initialized_data) => {
            // `initialized_data` 是 `solana_nonce::state::Data` 类型，包含了 Nonce 账户的详细信息。
            Ok(UiNonceState::Initialized(UiNonceData {
                authority: initialized_data.authority.to_string(), // 管理者地址转为字符串
                blockhash: initialized_data.blockhash().to_string(), // 当前存储的区块哈希转为字符串
                fee_calculator: initialized_data.fee_calculator.into(), // 原始 FeeCalculator 转为 UiFeeCalculator
            }))
        }
    }
}

// 枚举 `UiNonceState`:
// 这是 Nonce 账户状态的一个用户界面友好 (UI-friendly) 的副本/表示。
// 主要用于将解析后的 Nonce 账户状态序列化为易于阅读的 JSON 格式。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`:
//   - `Debug`: 允许使用 `{:?}` 格式化打印，方便调试。
//   - `Serialize`, `Deserialize`: 自动派生 `serde` 的序列化和反序列化能力。
//   - `PartialEq`, `Eq`: 允许比较此枚举的实例是否相等，主要用于测试。
// `#[serde(rename_all = "camelCase", tag = "type", content = "info")]`:
//   - `rename_all = "camelCase"`: 在序列化为 JSON 时，枚举成员名和字段名将转换为驼峰式命名。
//   - `tag = "type"`: JSON 对象会有一个 "type" 字段，其值为当前枚举成员的名称 (e.g., "initialized")。
//   - `content = "info"`: 对于包含数据的枚举成员（如 `Initialized(UiNonceData)`），其内部数据 (`UiNonceData`)
//     会序列化到一个名为 "info" 的 JSON 字段中。
// 例如，一个 `Initialized` 状态的 Nonce 账户会序列化成类似：
//   `{ "type": "initialized", "info": { "authority": "...", "blockhash": "...", "feeCalculator": { ... } } }`
// 而 `Uninitialized` (如果此解析器会返回它的话) 会是：
//   `{ "type": "uninitialized" }`
/// A duplicate representation of NonceState for pretty JSON serialization
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum UiNonceState {
    Uninitialized,             // 表示 Nonce 账户未初始化 (虽然当前 parse_nonce 函数逻辑不直接返回这个)
    Initialized(UiNonceData),  // 表示 Nonce 账户已初始化，并包含 `UiNonceData` 的详细信息
}

// 结构体 `UiNonceData`:
// 用于以用户友好的方式显示已初始化的 Nonce 账户的具体数据。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`: 派生常用特性。
// `#[serde(rename_all = "camelCase")]`: 序列化为 JSON 时字段名使用驼峰式。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiNonceData {
    // `authority`: Nonce 账户的管理者地址的字符串表示。
    pub authority: String,
    // `blockhash`: Nonce 账户当前存储的区块哈希的字符串表示。
    pub blockhash: String,
    // `fee_calculator`: 与此 Nonce 关联的交易费用计算器，使用 `UiFeeCalculator` 格式。
    pub fee_calculator: UiFeeCalculator,
}

// 测试模块
#[cfg(test)]
mod test {
    // 导入父模块的所有项 (`super::*`) 和测试中需要的特定项。
    use {
        super::*,
        solana_hash::Hash, // 导入 Solana 的哈希类型，用于表示区块哈希等。
        solana_nonce::{    // 从 Nonce 相关库导入。
            state::{Data as NonceAccountData, State as NonceCurrentState}, // `Data` 是已初始化 Nonce 账户的数据结构，`State` 是状态枚举。
                                                                        // 使用 `as` 重命名以避免与本模块或其他地方的同名类型冲突。
            versions::Versions as NonceAccountVersions, // Nonce 账户的版本管理结构。
        },
        solana_pubkey::Pubkey, // 导入公钥类型。
    };

    // 测试函数 `test_parse_nonce`
    // `#[test]` 宏标记这是一个单元测试函数。
    #[test]
    fn test_parse_nonce() {
        // 1. 测试一个正确初始化的 Nonce 账户的解析
        // 创建一个默认的已初始化 Nonce 状态数据。
        // `NonceAccountData::default()` 会创建一个包含默认管理者(Pubkey::default())、默认区块哈希(Hash::default())和默认费用计算器的 Nonce 数据。
        let nonce_data_internal = NonceAccountVersions::new(NonceCurrentState::Initialized(NonceAccountData::default()));
        // 使用 `bincode` 将这个状态序列化为字节向量，模拟链上存储的数据。
        let nonce_account_raw_data = bincode::serialize(&nonce_data_internal).unwrap();

        // 调用 `parse_nonce` 函数解析这些数据。
        // `.unwrap()` 用于获取 `Result` 中的 `Ok` 值，如果解析失败则测试会 panic。
        let parsed_result = parse_nonce(&nonce_account_raw_data).unwrap();

        // 断言：解析结果应该等于预期的 `UiNonceState::Initialized`，其内部包含转换后的默认值。
        assert_eq!(
            parsed_result,
            UiNonceState::Initialized(UiNonceData {
                authority: Pubkey::default().to_string(), // 默认管理者地址的字符串形式
                blockhash: Hash::default().to_string(),   // 默认区块哈希的字符串形式
                fee_calculator: UiFeeCalculator {         // 默认费用计算器的 UI 形式
                    lamports_per_signature: 0.to_string(), // 默认每签名费用为0
                },
            }),
        );

        // 2. 测试无效数据的情况
        let bad_data = vec![0; 4]; // 创建一些明显不是有效 Nonce 账户数据的字节 (长度也不符)。
        // 尝试用这些坏数据解析，并断言结果是 `Err`。
        // `.is_err()` 检查 `Result` 是否为 `Err` 变体。
        assert!(parse_nonce(&bad_data).is_err());

        // 3. 测试未初始化状态 (根据 parse_nonce 的逻辑，这应该返回错误)
        // let uninitialized_nonce_data = NonceAccountVersions::new(NonceCurrentState::Uninitialized);
        // let uninitialized_raw_data = bincode::serialize(&uninitialized_nonce_data).unwrap();
        // assert!(parse_nonce(&uninitialized_raw_data).is_err());
        // 注意：根据 `parse_nonce` 函数的实现，即使是正确序列化的 `State::Uninitialized` 也会被视为 `InvalidAccountData` 错误。
        // 如果需要显式测试这种情况，可以构造一个刚好符合 `State::Uninitialized` 序列化结果的数据，
        // 但更直接的是理解函数设计本身就不接受这种状态作为“可解析的Nonce”。
        // 通常，Nonce 账户在使用前必须被正确地初始化 (initialize_nonce_account 指令)。
    }
}
