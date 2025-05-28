// 文件功能说明：
// 这个文件负责解析 Solana 上的“配置账户”(Config Account)。
// 配置账户是由 `solana-config-program` (一个链上程序) 管理的特殊账户，用于存储各种链上配置信息。
// 其他程序或客户端可以读取这些配置账户来获取最新的系统参数或应用设定。
//
// 本文件主要处理两种类型的配置账户：
// 1. Stake Config Account (质押配置账户): 存储与质押机制相关的全局参数，例如质押激活和冷却的速率、
//    以及验证者因不当行为受到惩罚时被削减的质押比例等。这些参数对整个网络的经济激励和安全至关重要。
//    质押（Staking）是用户将他们的 SOL 代币委托给验证者，以帮助保护网络并分享验证者获得的奖励的过程。
// 2. Validator Info Account (验证者信息账户): 存储验证者节点向社区公开的元数据。
//    验证者是 Solana 网络中负责处理交易和生成新区块的关键参与者。
//    这个配置账户通常包含一个由验证者签名的 JSON 字符串，里面可以包含验证者的名称、网站链接、Logo、联系方式等。
//    这些信息有助于提高网络的透明度，方便用户选择和监督验证者。
//
// 此模块的代码会尝试根据账户的公钥 (地址) 和其包含的数据内容来识别它是哪种类型的配置账户，
// 并将二进制数据反序列化为用户友好的、结构化的 Rust 类型，方便上层应用展示和使用。

use {
    // 从当前 crate (solana-account-decoder) 导入错误类型和可解析账户枚举。
    crate::{
        parse_account_data::{ParsableAccount, ParseAccountError}, // `ParsableAccount` 用于标识账户类型，`ParseAccountError` 定义解析错误。
        validator_info, // 导入与验证者信息相关的定义，特别是 `validator_info::id()` 用于识别验证者信息账户。
    },
    bincode::deserialize, // 导入 `bincode` 库的 `deserialize` 函数，用于将二进制数据转换为 Rust 结构体。
    serde_json::Value,    // 导入 `serde_json` 库的 `Value` 类型，用于表示任意 JSON 数据。
    // 从 `solana-config-program-client` 导入与配置程序交互的工具。
    solana_config_program_client::{get_config_data, ConfigKeys}, // `get_config_data` 用于从配置账户数据中提取实际的配置字节。
                                                                // `ConfigKeys` 是配置账户中存储公钥列表的结构。
    solana_pubkey::Pubkey, // 导入 `Pubkey` 类型，表示 Solana 账户的公钥（地址）。
    // 从 `solana-stake-interface` 导入质押相关的配置定义。
    solana_stake_interface::config::{
        Config as StakeConfig, // `StakeConfig` 是链上质押配置的原始结构体。
        {self as stake_config}, // 将 `solana_stake_interface::config` 模块自身导入为 `stake_config`，方便调用 `stake_config::id()`。
    },
};

// 函数 `parse_config`：
// 功能：解析配置账户的原始二进制数据。
// 参数：
//   - `data`: `&[u8]`，一个字节切片，代表配置账户的原始数据。
//   - `pubkey`: `&Pubkey`，被解析的配置账户的公钥。这个公钥用于判断是哪种类型的配置账户（例如，质押配置账户有固定的公钥）。
// 返回值：
//   - `Result<ConfigAccountType, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(ConfigAccountType)`，其中 `ConfigAccountType` 是一个枚举，
//       包含了具体配置类型的信息（如 `StakeConfig` 或 `ValidatorInfo`）。
//     - 如果解析失败或无法识别账户类型，返回 `Err(ParseAccountError)`。
// 设计思路：
//   1. 首先，检查传入的 `pubkey` 是否等于已知的质押配置账户的ID (`stake_config::id()`)。
//      - 如果是质押配置账户：
//        a. 调用 `get_config_data(data)` 从原始账户数据中提取出实际的配置数据部分。
//           配置账户的数据通常包含一个 `ConfigKeys` 结构（描述哪些公钥参与了这个配置）和实际的配置字节。
//           `get_config_data` 会跳过 `ConfigKeys` 部分。
//        b. 如果成功提取数据，则尝试使用 `bincode::deserialize::<StakeConfig>(data)` 将其反序列化为 `StakeConfig` 结构体。
//        c. 如果反序列化成功，将原始的 `StakeConfig` 转换为用户友好的 `UiStakeConfig` (通过 `.into()` 实现)，
//           然后包装在 `ConfigAccountType::StakeConfig` 中。
//        d. 这一系列操作使用了 `Option` 的 `and_then` 和 `map` 方法进行链式处理，如果任何一步失败（返回 `None` 或 `Err`），
//           整个表达式的结果也会是 `None`。
//   2. 如果 `pubkey` 不是质押配置账户的ID，则尝试将其作为通用的配置账户（目前主要是验证者信息账户）进行解析：
//      a. 尝试使用 `bincode::deserialize::<ConfigKeys>(data)` 从原始数据开头反序列化出 `ConfigKeys` 结构。
//         `ConfigKeys` 包含一个公钥列表 `keys`，其中每个元素是一个元组 `(Pubkey, bool)`，`bool` 值表示该公钥是否是签名者。
//      b. 如果成功反序列化 `ConfigKeys`，并且 `key_list.keys` 不为空，并且第一个公钥 `key_list.keys[0].0`
//         等于已知的验证者信息账户的标识 `validator_info::id()`：
//         i. 这表明它可能是一个验证者信息账户。调用 `parse_config_data::<String>(data, key_list.keys)`
//            来进一步解析。`parse_config_data` 是一个辅助函数，它会提取实际的配置数据（这里期望是字符串类型，即 JSON 字符串）
//            并将其与 `keys` 一起包装进 `UiConfig` 结构。
//         ii. 如果 `parse_config_data` 成功，并且返回的 `validator_info.config_data` (JSON 字符串)
//             能够被 `serde_json::from_str` 成功解析为 `serde_json::Value` (通用的 JSON 对象)，
//             则构造一个 `ConfigAccountType::ValidatorInfo`，其中包含转换后的 `keys` 和解析后的 `config_data` (作为 `Value`)。
//         iii. `and_then` 和 `?` (try 操作符) 用于优雅地处理可能失败的步骤。
//      c. 如果以上条件不满足（例如 `keys` 为空，或第一个 key 不是 `validator_info::id()`），则返回 `None`。
//   3. 最后，`parsed_account` 是一个 `Option<ConfigAccountType>`。
//      - 如果它是 `Some(account_type)`，则表示解析成功，返回 `Ok(account_type)`。
//      - 如果它是 `None`（表示无法识别或解析失败），则返回 `Err(ParseAccountError::AccountNotParsable(ParsableAccount::Config))`。
pub fn parse_config(data: &[u8], pubkey: &Pubkey) -> Result<ConfigAccountType, ParseAccountError> {
    // 尝试解析账户数据
    let parsed_account = if pubkey == &stake_config::id() {
        // 情况1: 如果账户公钥是质押配置账户的固定ID
        get_config_data(data) // 从原始数据中提取配置字节 (跳过 ConfigKeys)
            .ok() // 转换为 Option<Vec<u8>>
            .and_then(|config_bytes| deserialize::<StakeConfig>(config_bytes).ok()) // 尝试将配置字节反序列化为 StakeConfig
            .map(|config| ConfigAccountType::StakeConfig(config.into())) // 如果成功，转换为 UiStakeConfig 并包装
    } else {
        // 情况2: 尝试作为其他类型的配置账户解析 (主要是验证者信息账户)
        deserialize::<ConfigKeys>(data).ok().and_then(|key_list| { // 先尝试反序列化 ConfigKeys 结构
            // 检查 ConfigKeys 是否表明这是一个验证者信息账户
            if !key_list.keys.is_empty() && key_list.keys[0].0 == validator_info::id() {
                // 如果是，调用辅助函数 parse_config_data 来解析数据部分 (期望是 String 类型的 JSON)
                parse_config_data::<String>(data, key_list.keys).and_then(|validator_config_string| {
                    // 进一步将解析出的 JSON 字符串转换为 serde_json::Value
                    let ui_config_value = serde_json::from_str(&validator_config_string.config_data).ok()?;
                    Some(ConfigAccountType::ValidatorInfo(UiConfig { // 构造 ValidatorInfo 类型的返回结果
                        keys: validator_config_string.keys,
                        config_data: ui_config_value,
                    }))
                })
            } else {
                // 如果 ConfigKeys 不符合验证者信息账户的特征，则认为无法解析
                None
            }
        })
    };

    // 根据解析结果返回 Ok 或 Err
    // `ok_or` 是 `Option` 的一个方法：如果 `parsed_account` 是 `Some(value)`，它返回 `Ok(value)`；
    // 如果是 `None`，它返回 `Err`，其内容由传入的闭包或值决定。
    parsed_account.ok_or(ParseAccountError::AccountNotParsable(
        ParsableAccount::Config, // 如果无法解析，返回此错误，指明是 Config 类型的账户解析失败
    ))
}

// 辅助函数 `parse_config_data`：
// 功能：从配置账户的原始数据中提取并反序列化实际的配置内容，同时转换公钥列表的格式。
// 泛型参数 `T`: 表示期望的配置数据类型，例如 `String` (用于验证者信息的 JSON 字符串) 或 `StakeConfig`。
//             `T` 必须实现 `serde::de::DeserializeOwned` 特性，意味着它可以从序列化的数据中创建。
// 参数：
//   - `data`: `&[u8]`，配置账户的完整原始数据。
//   - `keys`: `Vec<(Pubkey, bool)>`，从账户数据中解析出来的 `ConfigKeys` 中的公钥列表。
// 返回值：
//   - `Option<UiConfig<T>>`: 如果成功提取并反序列化配置数据，则返回 `Some(UiConfig<T>)`，
//     其中 `UiConfig` 包含了用户友好的公钥列表和类型为 `T` 的配置数据。否则返回 `None`。
// Rust 特有概念：
//   - `where T: serde::de::DeserializeOwned`: 这是一个泛型约束，表示类型 `T` 必须能够被反序列化。
//     `DeserializeOwned` 意味着类型 `T` 在反序列化过程中不依赖于输入数据的生命周期（即它拥有所有反序列化出来的数据）。
fn parse_config_data<T>(data: &[u8], keys: Vec<(Pubkey, bool)>) -> Option<UiConfig<T>>
where
    T: serde::de::DeserializeOwned, // 泛型约束：T 必须可以被反序列化
{
    // 1. 提取实际的配置数据字节：
    //    `get_config_data(data)` 会跳过数据开头存储 `ConfigKeys` 的部分，返回剩下的字节。
    //    `.ok()?` 如果 `get_config_data` 返回 `Err` 或 `None`，则此函数立即返回 `None` (通过 `?` 操作符)。
    let config_bytes = get_config_data(data).ok()?;

    // 2. 反序列化配置数据字节为指定的类型 `T`：
    //    `deserialize::<T>(config_bytes)` 尝试将这些字节反序列化。
    //    `.ok()?` 如果反序列化失败，则此函数立即返回 `None`。
    let config_data_typed: T = deserialize(config_bytes).ok()?;

    // 3. 转换公钥列表的格式：
    //    原始的 `keys` 是 `Vec<(Pubkey, bool)>`。
    //    使用 `.iter().map(...).collect()` 将其转换为 `Vec<UiConfigKey>`。
    //    `UiConfigKey` 将 `Pubkey` 转换为字符串，更方便显示。
    let ui_keys = keys
        .iter() // 创建迭代器
        .map(|key_tuple| UiConfigKey { //对每个元组 `(Pubkey, bool)` 进行转换
            pubkey: key_tuple.0.to_string(), // Pubkey 转换为 String
            signer: key_tuple.1,             // bool 值 (是否为签名者) 直接使用
        })
        .collect(); // 将转换后的 `UiConfigKey` 收集到一个新的 Vec 中

    // 4. 构建并返回 `UiConfig<T>` 结构体，包装在 `Some` 中。
    Some(UiConfig { keys: ui_keys, config_data: config_data_typed })
}

// 枚举 `ConfigAccountType`:
// 表示已解析的配置账户的具体类型。
// `#[derive(Debug, Serialize, Deserialize, PartialEq)]`:
//   - `Debug`: 允许调试打印。
//   - `Serialize`, `Deserialize`: 允许 `serde` 进行序列化和反序列化。
//   - `PartialEq`: 允许比较实例是否相等（主要用于测试）。
// `#[serde(rename_all = "camelCase", tag = "type", content = "info")]`:
//   - `rename_all = "camelCase"`: JSON 字段和枚举成员名使用驼峰式。
//   - `tag = "type"`: JSON 中会有一个 "type" 字段，值为枚举成员名 (e.g., "stakeConfig")。
//   - `content = "info"`: 枚举成员关联的数据会放在 "info" 字段中。
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum ConfigAccountType {
    StakeConfig(UiStakeConfig),       // 表示这是一个质押配置账户，包含 `UiStakeConfig` 数据。
    ValidatorInfo(UiConfig<Value>),   // 表示这是一个验证者信息账户，包含 `UiConfig<Value>` 数据。
                                      // `Value` 来自 `serde_json`，代表任意 JSON 值，因为验证者信息是 JSON 格式。
}

// 结构体 `UiConfigKey`:
// 用于以用户友好的方式表示配置账户中的一个公钥及其签名状态。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`:
//   - `Eq` 是 `PartialEq` 的增强版，表示全等关系。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfigKey {
    pub pubkey: String, // 公钥的字符串表示。
    pub signer: bool,   // 布尔值，指示此公钥是否是创建或修改此配置的签名者之一。
}

// 结构体 `UiStakeConfig`:
// 用于以用户友好的方式显示质押配置信息。
// `#[deprecated(...)]`: 标记此结构体中的某些字段或整个结构体可能已不推荐使用。
//   `warmup_cooldown_rate` 字段的获取方式已有更新，推荐使用 `solana_stake_interface::state::warmup_cooldown_rate()`。
#[deprecated(
    since = "1.16.7",
    note = "Please use `solana_stake_interface::state::warmup_cooldown_rate()` instead"
)]
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStakeConfig {
    // `warmup_cooldown_rate`: 质押激活（预热）和停用（冷却）的速率。
    // 这是一个小数，表示每个 epoch (Solana 中的一个时间周期，约2-3天) 可以激活或停用多少比例的质押。
    pub warmup_cooldown_rate: f64,
    // `slash_penalty`: 对行为不当的验证者进行惩罚时，削减其质押金的百分比。
    // 例如，如果值为 50，表示削减 50%。这是一个 `u8` 类型，范围 0-100。
    pub slash_penalty: u8,
}

// 实现 `From<StakeConfig> for UiStakeConfig`:
// 定义如何从原始的链上 `StakeConfig` 结构体转换为用户友好的 `UiStakeConfig`。
// Rust 特有概念：
//   - `impl From<T> for U`: 这是实现 `From` 特性的标准语法。如果实现了 `From<T> for U`，
//     那么 Rust 会自动提供 `Into<U> for T` 的实现，允许使用 `.into()` 进行转换。
impl From<StakeConfig> for UiStakeConfig {
    fn from(config: StakeConfig) -> Self { // `config` 是输入的原始 `StakeConfig`
        Self { // `Self` 指代 `UiStakeConfig`
            warmup_cooldown_rate: config.warmup_cooldown_rate, // 直接复制字段值
            slash_penalty: config.slash_penalty,
        }
    }
}

// 结构体 `UiConfig<T>`:
// 一个通用的、用户友好的配置结构体。
// 泛型参数 `T`: 代表实际配置数据的类型。例如，对于验证者信息，`T` 会是 `serde_json::Value`。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfig<T> {
    // `keys`: 一个 `Vec<UiConfigKey>`，包含了与此配置相关的公钥列表及其签名状态。
    pub keys: Vec<UiConfigKey>,
    // `config_data`: 实际的配置数据，其类型由泛型参数 `T` 决定。
    pub config_data: T,
}

// 测试模块
#[cfg(test)]
mod test {
    // 导入父模块的所有项 (`super::*`) 和测试中需要的特定项。
    use {
        super::*,
        crate::validator_info::ValidatorInfo, // 导入验证者信息结构体，用于测试。
        bincode::serialize,                  // 用于在测试中序列化数据以模拟链上状态。
        serde_json::json,                    // `serde_json::json!` 宏方便创建 JSON 值。
        solana_account::{Account, AccountSharedData, ReadableAccount}, // Solana 账户相关的结构。
        solana_config_program_client::ConfigKeys, // 配置键结构。
    };

    // 辅助函数 `create_config_account`：
    // 功能：创建一个模拟的配置账户 (`AccountSharedData`)，用于测试。
    // 泛型参数 `T`: 必须实现 `serde::Serialize`，以便可以被 `bincode` 序列化。
    // 参数：
    //   - `keys`: `Vec<(Pubkey, bool)>`，要存储在账户 `ConfigKeys` 部分的公钥列表。
    //   - `config_data`: `&T`，要序列化并存储为实际配置数据的对象。
    //   - `lamports`: `u64`，账户的余额。
    // 返回值：
    //   - `AccountSharedData`，一个模拟的链上账户。
    fn create_config_account<T: serde::Serialize>(
        keys: Vec<(Pubkey, bool)>,
        config_data: &T,
        lamports: u64,
    ) -> AccountSharedData {
        // 1. 序列化 `ConfigKeys` 结构。
        let mut data = serialize(&ConfigKeys { keys }).unwrap();
        // 2. 序列化实际的配置数据 `config_data`，并将其字节附加到 `data` 后面。
        data.extend_from_slice(&serialize(config_data).unwrap());
        // 3. 创建并返回一个 `AccountSharedData` 实例。
        AccountSharedData::from(Account {
            lamports,                               // 设置账户余额。
            data,                                   // 设置账户数据。
            owner: solana_sdk_ids::config::id(),    // 设置账户的所有者为配置程序的ID。
            ..Account::default()                    // 其余字段使用 `Account` 的默认值。
        })
    }

    // 测试函数 `test_parse_config`
    // `#[test]` 宏标记这是一个单元测试函数。
    #[test]
    fn test_parse_config() {
        // 1. 测试质押配置账户的解析
        let stake_config_data = StakeConfig { // 创建一个示例 StakeConfig
            warmup_cooldown_rate: 0.25,
            slash_penalty: 50,
        };
        // 使用辅助函数创建一个模拟的质押配置账户。
        // 质押配置账户通常没有额外的 `keys`（因为它由固定的ID标识），所以传入空 `vec![]`。
        let stake_config_account = create_config_account(vec![], &stake_config_data, 10);
        // 调用 `parse_config` 进行解析。
        // `stake_config_account.data()` 获取账户的原始数据。
        // `&stake_config::id()` 传入质押配置账户的公钥。
        // `.unwrap()` 用于获取 `Result` 中的 `Ok` 值，如果解析失败则测试会 panic。
        assert_eq!( // 断言解析结果是否符合预期。
            parse_config(stake_config_account.data(), &stake_config::id()).unwrap(),
            ConfigAccountType::StakeConfig(UiStakeConfig { // 预期的解析结果
                warmup_cooldown_rate: 0.25,
                slash_penalty: 50,
            }),
        );

        // 2. 测试验证者信息账户的解析
        let validator_info_data = ValidatorInfo { // 创建一个示例 ValidatorInfo
            info: serde_json::to_string(&json!({ // `info` 字段是一个 JSON 字符串
                "name": "Solana",
            }))
            .unwrap(),
        };
        let info_pubkey = solana_pubkey::new_rand(); // 为此验证者信息账户创建一个随机公钥。
        // 创建模拟的验证者信息账户。
        // `keys` 列表通常包含 `validator_info::id()` 和验证者自己的公钥（作为签名者）。
        let validator_info_config_account = create_config_account(
            vec![(validator_info::id(), false), (info_pubkey, true)], // `keys`
            &validator_info_data, // 实际配置数据
            10,
        );
        assert_eq!( // 断言解析结果
            parse_config(validator_info_config_account.data(), &info_pubkey).unwrap(),
            ConfigAccountType::ValidatorInfo(UiConfig { // 预期的解析结果
                keys: vec![ // 预期的公钥列表
                    UiConfigKey {
                        pubkey: validator_info::id().to_string(),
                        signer: false,
                    },
                    UiConfigKey {
                        pubkey: info_pubkey.to_string(),
                        signer: true,
                    }
                ],
                config_data: serde_json::from_str(r#"{"name":"Solana"}"#).unwrap(), // 预期的 JSON 数据
            }),
        );

        // 3. 测试无效数据的情况
        let bad_data = vec![0; 4]; // 创建一些明显不是有效配置账户数据的字节。
        // 尝试用这些坏数据解析，并断言结果是 `Err`。
        // `.is_err()` 检查 `Result` 是否为 `Err` 变体。
        assert!(parse_config(&bad_data, &info_pubkey).is_err());
    }
}
