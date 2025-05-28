// 文件功能说明：
// 这个文件专门负责解析 Solana 上的质押账户（Stake Account）数据。
// 在 Solana 网络中，质押（Staking）是网络安全和共识机制的核心组成部分。
// SOL 代币的持有者可以将他们的 SOL “委托”给网络上的验证者（Validators）。
// 验证者是运行 Solana 节点、处理交易、生成新区块并参与共识的实体。
// 通过将 SOL 委托给验证者，持有者不仅可以帮助保护网络，还可以根据其质押的数额分享网络发放的奖励。
//
// 质押账户有几种不同的状态，本文件会解析这些状态并将其转换为用户友好的格式：
// 1. Uninitialized: 账户已在链上创建，但尚未被初始化为一个有效的质押账户。
// 2. Initialized: 账户已被初始化，可以接收 SOL，但尚未将这些 SOL 委托给任何验证者。
//    此时，账户会定义谁有权进行质押操作（staker）和谁有权提取资金（withdrawer）。
// 3. Delegated:  账户中的 SOL 已经委托给了一个特定的验证者。这种状态下，账户会记录委托给哪个验证者、
//    委托了多少 SOL、委托何时开始激活（可以计算奖励）以及何时开始停用（准备提取）。
// 4. RewardsPool: 这是一个内部状态，与整个网络的质押奖励分配机制相关，普通用户通常不直接操作此状态的账户。
//
// 质押账户还包含以下关键信息：
// - Meta (元数据):
//   - rent_exempt_reserve: 为了使账户在链上永久存在而无需支付持续的租金（rent）所必须保留的最小 SOL 数量。
//   - authorized: 定义了两个关键的授权公钥：
//     - staker: 有权执行质押相关操作（如选择验证者进行委托、取消委托）的账户地址。
//     - withdrawer: 有权从该质押账户中提取未质押的 SOL 和已累积的奖励的账户地址。
//   - lockup (锁仓): 定义了账户中资金的锁仓规则，例如在某个特定的Unix时间戳或某个特定的 Epoch (时期) 之前，
//     资金不能被提取或转移。这常用于投资归属计划或确保长期质押。
// - Stake (质押详情，仅在 Delegated 状态下存在):
//   - delegation: 描述了当前委托的具体情况：
//     - voter_pubkey: 被委托的验证者的投票账户的公钥。每个验证者都有一个投票账户用于参与共识投票。
//     - stake: 当前已激活并正在参与质押的 SOL 数量。
//     - activation_epoch: 此委托开始激活并有资格获得奖励的 Epoch。Epoch 是 Solana 中的一个时间单位，大致相当于一个区块生成周期或一小段时间。
//     - deactivation_epoch: 如果此委托被设置为停用，这是它开始进入停用过程的 Epoch。停用后，SOL 不再获得奖励，并需要一段时间才能完全提取。
//     - warmup_cooldown_rate: (已废弃字段) 之前用于描述质押激活和冷却速率的参数。
//   - credits_observed: 一个与奖励计算相关的内部字段，大致记录了此质押账户观察到的、由其委托的验证者产生的“信用点数”。
//                     验证者处理的交易越多、投票越积极，产生的信用点数就越多，从而可能获得更多奖励。
//
// 本文件中的代码通过反序列化质押账户的原始二进制数据，将其转换为一系列易于理解的 Rust 结构体，
// 这些结构体随后可以被序列化为 JSON 等格式，供钱包、区块链浏览器等工具向用户展示质押账户的详细信息。

use {
    // 从当前 crate (solana-account-decoder) 导入错误类型、可解析账户枚举和用于表示金额的字符串类型。
    crate::{
        parse_account_data::{ParsableAccount, ParseAccountError}, // `ParsableAccount` 用于标识账户类型，`ParseAccountError` 定义解析错误。
        StringAmount, // 通常是 `String` 的类型别名，用于表示代币数量，以避免浮点数精度问题。
    },
    bincode::deserialize, // 导入 `bincode` 库的 `deserialize` 函数，用于将二进制数据转换为 Rust 结构体。
    solana_clock::{Epoch, UnixTimestamp}, // 从 Solana 时钟库导入 `Epoch` (时期) 和 `UnixTimestamp` (Unix时间戳) 类型。
                                          // Epoch 是 Solana 网络中的一个时间概念，代表一定数量的 slot (槽，即区块)。
                                          // UnixTimestamp 是标准的Unix时间戳，表示从1970年1月1日以来的秒数。
    // 从 Solana 质押程序接口库导入质押账户相关的状态定义。
    solana_stake_interface::state::{Authorized, Delegation, Lockup, Meta, Stake, StakeStateV2},
    // `Authorized`: 包含 staker 和 withdrawer 公钥的结构体。
    // `Delegation`: 描述质押委托详情的结构体。
    // `Lockup`: 描述锁仓设置的结构体。
    // `Meta`: 质押账户的元数据结构体。
    // `Stake`: 包含委托和信用点数的结构体。
    // `StakeStateV2`: 表示质押账户不同状态的枚举 (这是链上存储的原始状态)。
};

// 函数 `parse_stake`：
// 功能：解析质押账户的原始二进制数据。
// 参数：
//   - `data`: `&[u8]`，一个字节切片，代表质押账户的原始数据。
// 返回值：
//   - `Result<StakeAccountType, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(StakeAccountType)`，其中 `StakeAccountType` 是一个用户友好的枚举，
//       表示质押账户的状态及其详细信息。
//     - 如果解析失败（例如数据格式不正确），返回 `Err(ParseAccountError)`。
// 设计思路：
//   1. 首先，尝试使用 `bincode::deserialize(data)` 将传入的原始字节数据 `data` 反序列化为 `StakeStateV2` 枚举。
//      `StakeStateV2` 是链上质押账户状态的原始表示。
//      如果反序列化失败，说明数据不是一个有效的质押账户状态，此时映射错误为
//      `ParseAccountError::AccountNotParsable(ParsableAccount::Stake)` 并返回。
//      `.map_err(|_| ...)` 是 `Result` 的一个方法，用于在 `Err` 的情况下转换错误类型。`_` 忽略原始的 bincode 错误。
//   2. 如果反序列化成功，我们得到了 `stake_state`（一个 `StakeStateV2` 的实例）。
//   3. 接下来，使用 `match` 表达式根据 `stake_state` 的不同成员（即账户的不同状态）进行处理，
//      将其转换为用户友好的 `StakeAccountType` 枚举：
//      - `StakeStateV2::Uninitialized`: 直接映射为 `StakeAccountType::Uninitialized`。
//      - `StakeStateV2::Initialized(meta)`:
//        - `meta` 是原始的 `Meta` 结构体。
//        - 将 `meta` 转换为用户友好的 `UiMeta` 格式 (通过 `.into()`，因为 `UiMeta` 实现了 `From<Meta>`)。
//        - 构建一个 `UiStakeAccount`，其中只包含 `meta` 信息，`stake` 字段为 `None` (因为此时还未委托)。
//        - 将这个 `UiStakeAccount` 包装在 `StakeAccountType::Initialized` 中。
//      - `StakeStateV2::Stake(meta, stake, _)`: (注意：第三个参数 `StakeFlags` 在这里被忽略了，用 `_` 表示)
//        - `meta` 是原始的 `Meta` 结构体，`stake` 是原始的 `Stake` 结构体。
//        - 将 `meta` 转换为 `UiMeta`。
//        - 将 `stake` 转换为用户友好的 `UiStake` 格式 (通过 `.into()`，因为 `UiStake` 实现了 `From<Stake>`)。
//        - 构建一个 `UiStakeAccount`，同时包含 `meta` 和 `stake` (包装在 `Some(...)`中)。
//        - 将这个 `UiStakeAccount` 包装在 `StakeAccountType::Delegated` 中。
//      - `StakeStateV2::RewardsPool`: 直接映射为 `StakeAccountType::RewardsPool`。
//   4. 将匹配和转换后得到的 `parsed_account` (类型为 `StakeAccountType`) 包装在 `Ok()` 中并返回。
pub fn parse_stake(data: &[u8]) -> Result<StakeAccountType, ParseAccountError> {
    // 1. 尝试将原始数据反序列化为链上状态 StakeStateV2
    let stake_state: StakeStateV2 = deserialize(data)
        .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::Stake))?; // 如果失败，返回解析错误

    // 2. 根据反序列化得到的账户状态，匹配并转换为用户友好的 StakeAccountType
    let parsed_account = match stake_state {
        StakeStateV2::Uninitialized => StakeAccountType::Uninitialized, // 未初始化状态
        StakeStateV2::Initialized(meta) => StakeAccountType::Initialized(UiStakeAccount { // 已初始化状态
            meta: meta.into(), // 将原始 Meta 转换为 UiMeta
            stake: None,       // 此时没有质押委托信息
        }),
        StakeStateV2::Stake(meta, stake, _stake_flags) => StakeAccountType::Delegated(UiStakeAccount { // 已委托状态
            // _stake_flags (第三个参数) 在这里被忽略
            meta: meta.into(),           // 将原始 Meta 转换为 UiMeta
            stake: Some(stake.into()), // 将原始 Stake 转换为 UiStake，并包装在 Option::Some 中
        }),
        StakeStateV2::RewardsPool => StakeAccountType::RewardsPool, // 奖励池状态
    };
    // 3. 返回成功解析的结果
    Ok(parsed_account)
}

// 枚举 `StakeAccountType`:
// 表示质押账户的用户友好状态类型。
// `#[derive(Debug, Serialize, Deserialize, PartialEq)]`:
//   - `Debug`: 允许使用 `{:?}` 格式化打印，方便调试。
//   - `Serialize`, `Deserialize`: 自动派生 `serde` 的序列化和反序列化能力，使其可以轻松转换为 JSON 等格式。
//   - `PartialEq`: 允许比较此枚举的实例是否相等，主要用于测试。
// `#[serde(rename_all = "camelCase", tag = "type", content = "info")]`:
//   - `rename_all = "camelCase"`: 在序列化为 JSON 时，枚举成员名和字段名将转换为驼峰式命名。
//   - `tag = "type"`: JSON 对象会有一个 "type" 字段，其值为当前枚举成员的名称 (e.g., "initialized", "delegated")。
//   - `content = "info"`: 对于包含数据的枚举成员（如 `Initialized(UiStakeAccount)`），其内部数据 (`UiStakeAccount`)
//     会序列化到一个名为 "info" 的 JSON 字段中。
// 例如，一个 `Delegated` 状态的质押账户会序列化成类似：
//   `{ "type": "delegated", "info": { "meta": { ... }, "stake": { ... } } }`
// 而 `Uninitialized` 会是：
//   `{ "type": "uninitialized" }`
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum StakeAccountType {
    Uninitialized,              // 未初始化
    Initialized(UiStakeAccount), // 已初始化，包含账户元数据但未委托
    Delegated(UiStakeAccount),   // 已委托，包含账户元数据和委托详情
    RewardsPool,                // 奖励池（内部状态）
}

// 结构体 `UiStakeAccount`:
// 用户友好的质押账户表示，包含了元数据和可选的质押信息。
// `#[derive(Debug, Serialize, Deserialize, PartialEq)]`: 派生常用特性。
// `#[serde(rename_all = "camelCase")]`: 序列化为 JSON 时字段名使用驼峰式。
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStakeAccount {
    // `meta`: 质押账户的元数据，使用 `UiMeta` 结构表示。
    pub meta: UiMeta,
    // `stake`: 可选的质押详情，使用 `UiStake` 结构表示。
    // 如果账户是 `Initialized` 但未委托，或者不是一个有效的委托账户，则此字段为 `None`。
    // 如果账户是 `Delegated`，则此字段为 `Some(UiStake)`。
    pub stake: Option<UiStake>,
}

// 结构体 `UiMeta`:
// 用户友好的质押账户元数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`:
//   - `Eq` 是 `PartialEq` 的增强版，表示全等关系（所有字段都必须实现 `Eq`）。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMeta {
    // `rent_exempt_reserve`: 免租金保留额，以字符串形式表示的 lamports 数量。
    // Lamport 是 SOL 的最小单位 (1 SOL = 1,000,000,000 lamports)。
    pub rent_exempt_reserve: StringAmount,
    // `authorized`: 授权信息，包含 staker 和 withdrawer 的公钥，使用 `UiAuthorized` 结构表示。
    pub authorized: UiAuthorized,
    // `lockup`: 锁仓设置，使用 `UiLockup` 结构表示。
    pub lockup: UiLockup,
}

// 实现 `From<Meta> for UiMeta`:
// 定义如何从原始的链上 `Meta` 结构体转换为用户友好的 `UiMeta`。
impl From<Meta> for UiMeta {
    fn from(meta: Meta) -> Self { // `meta` 是输入的原始 `Meta`
        Self { // `Self` 指代 `UiMeta`
            rent_exempt_reserve: meta.rent_exempt_reserve.to_string(), // u64 转 String
            authorized: meta.authorized.into(), // 原始 Authorized 转 UiAuthorized (需要 UiAuthorized 实现 From<Authorized>)
            lockup: meta.lockup.into(),         // 原始 Lockup 转 UiLockup (需要 UiLockup 实现 From<Lockup>)
        }
    }
}

// 结构体 `UiLockup`:
// 用户友好的锁仓设置表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiLockup {
    // `unix_timestamp`: 锁仓截止的 Unix 时间戳。在该时间点之前，资金可能无法提取。
    // 值为 0 表示没有基于时间的锁仓。
    pub unix_timestamp: UnixTimestamp,
    // `epoch`: 锁仓截止的 Epoch (时期) 号码。在该 Epoch 到达之前，资金可能无法提取。
    // 值为 0 表示没有基于 Epoch 的锁仓。
    pub epoch: Epoch,
    // `custodian`: 锁仓的管理者或“保管人”的公钥字符串。
    // 在某些复杂的锁仓设置中，可能需要保管人的额外操作才能解除锁仓。
    // 对于简单的时间锁仓，它通常是全零的公钥。
    pub custodian: String,
}

// 实现 `From<Lockup> for UiLockup`:
// 定义如何从原始的链上 `Lockup` 结构体转换为用户友好的 `UiLockup`。
impl From<Lockup> for UiLockup {
    fn from(lockup: Lockup) -> Self {
        Self {
            unix_timestamp: lockup.unix_timestamp, // 直接复制
            epoch: lockup.epoch,                   // 直接复制
            custodian: lockup.custodian.to_string(), // Pubkey 转 String
        }
    }
}

// 结构体 `UiAuthorized`:
// 用户友好的授权公钥表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiAuthorized {
    // `staker`: 有权进行质押操作（如委托、解除委托、分割账户等）的账户公钥字符串。
    pub staker: String,
    // `withdrawer`: 有权提取账户中未质押的 SOL 和奖励的账户公钥字符串。
    pub withdrawer: String,
}

// 实现 `From<Authorized> for UiAuthorized`:
// 定义如何从原始的链上 `Authorized` 结构体转换为用户友好的 `UiAuthorized`。
impl From<Authorized> for UiAuthorized {
    fn from(authorized: Authorized) -> Self {
        Self {
            staker: authorized.staker.to_string(),         // Pubkey 转 String
            withdrawer: authorized.withdrawer.to_string(), // Pubkey 转 String
        }
    }
}

// 结构体 `UiStake`:
// 用户友好的质押详情表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStake {
    // `delegation`: 当前的委托信息，使用 `UiDelegation` 结构表示。
    pub delegation: UiDelegation,
    // `credits_observed`: 观察到的信用点数。这是一个 u64 值，反映了验证者活动的度量，与奖励计算相关。
    pub credits_observed: u64,
}

// 实现 `From<Stake> for UiStake`:
// 定义如何从原始的链上 `Stake` 结构体转换为用户友好的 `UiStake`。
impl From<Stake> for UiStake {
    fn from(stake: Stake) -> Self {
        Self {
            delegation: stake.delegation.into(), // 原始 Delegation 转 UiDelegation
            credits_observed: stake.credits_observed, // 直接复制
        }
    }
}

// 结构体 `UiDelegation`:
// 用户友好的委托详情表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiDelegation {
    // `voter`: 被委托的验证者的投票账户公钥字符串。
    pub voter: String,
    // `stake`: 当前激活并参与质押的 SOL 数量（以 lamports 为单位），表示为字符串。
    pub stake: StringAmount,
    // `activation_epoch`: 此委托开始激活并计算奖励的 Epoch (时期) 号码，表示为字符串。
    pub activation_epoch: StringAmount, // 虽然是 Epoch (u64)，但通常也用 StringAmount 显示
    // `deactivation_epoch`: 此委托开始停用的 Epoch (时期) 号码，表示为字符串。
    // 如果尚未停用，通常是 `u64::MAX`。
    pub deactivation_epoch: StringAmount,
    // `warmup_cooldown_rate`: (已废弃字段) 质押激活和冷却的速率。
    // `#[deprecated(...)]` 属性宏标记此字段已不推荐使用。
    #[deprecated(
        since = "1.16.7",
        note = "Please use `solana_stake_interface::state::warmup_cooldown_rate()` instead"
    )]
    pub warmup_cooldown_rate: f64,
}

// 实现 `From<Delegation> for UiDelegation`:
// 定义如何从原始的链上 `Delegation` 结构体转换为用户友好的 `UiDelegation`。
impl From<Delegation> for UiDelegation {
    fn from(delegation: Delegation) -> Self {
        // `#[allow(deprecated)]` 属性宏允许在函数体内部使用被标记为 deprecated 的字段（如此处的 `warmup_cooldown_rate`）。
        #[allow(deprecated)]
        Self {
            voter: delegation.voter_pubkey.to_string(), // Pubkey 转 String
            stake: delegation.stake.to_string(),         // u64 转 String (lamports)
            activation_epoch: delegation.activation_epoch.to_string(), // u64 (Epoch) 转 String
            deactivation_epoch: delegation.deactivation_epoch.to_string(), // u64 (Epoch) 转 String
            warmup_cooldown_rate: delegation.warmup_cooldown_rate, // 直接复制 (f64)
        }
    }
}

// 测试模块
#[cfg(test)]
mod test {
    // 导入父模块的所有项 (`super::*`) 和测试中需要的特定项。
    use {super::*, bincode::serialize, solana_stake_interface::stake_flags::StakeFlags};
    // `StakeFlags` 是一个用于表示质押账户额外标志的结构，在主解析逻辑中被忽略，但在构造测试数据时可能需要。

    // 测试函数 `test_parse_stake`
    // `#[test]` 宏标记这是一个单元测试函数。
    // `#[allow(deprecated)]` 允许在测试函数内部使用被标记为 deprecated 的字段或函数。
    #[test]
    #[allow(deprecated)]
    fn test_parse_stake() {
        // 1. 测试 Uninitialized 状态
        let stake_state_uninit = StakeStateV2::Uninitialized;
        // 将状态序列化为字节数据
        let stake_data_uninit = serialize(&stake_state_uninit).unwrap();
        // 解析并断言结果
        assert_eq!(
            parse_stake(&stake_data_uninit).unwrap(),
            StakeAccountType::Uninitialized
        );

        // 准备一些通用的元数据用于后续的 Initialized 和 Delegated 状态测试
        let staker_withdrawer_pubkey = solana_pubkey::new_rand(); // 随机生成一个公钥作为 staker 和 withdrawer
        let custodian_pubkey = solana_pubkey::new_rand();      // 随机生成一个公钥作为锁仓的 custodian
        let authorized_meta = Authorized::auto(&staker_withdrawer_pubkey); // 创建授权信息，staker 和 withdrawer 相同
        let lockup_meta = Lockup { // 创建锁仓信息
            unix_timestamp: 0,   // 无时间锁
            epoch: 1,            // 锁到 epoch 1
            custodian: custodian_pubkey,
        };
        let meta_data = Meta { // 创建元数据
            rent_exempt_reserve: 42, // 免租金保留额
            authorized: authorized_meta,
            lockup: lockup_meta,
        };

        // 2. 测试 Initialized 状态
        let stake_state_init = StakeStateV2::Initialized(meta_data); // 使用上面的元数据创建 Initialized 状态
        let stake_data_init = serialize(&stake_state_init).unwrap();
        assert_eq!(
            parse_stake(&stake_data_init).unwrap(),
            StakeAccountType::Initialized(UiStakeAccount {
                meta: UiMeta { // 预期的 UiMeta 结构
                    rent_exempt_reserve: 42.to_string(),
                    authorized: UiAuthorized {
                        staker: staker_withdrawer_pubkey.to_string(),
                        withdrawer: staker_withdrawer_pubkey.to_string(),
                    },
                    lockup: UiLockup {
                        unix_timestamp: 0,
                        epoch: 1,
                        custodian: custodian_pubkey.to_string(),
                    }
                },
                stake: None, // Initialized 状态下 stake 字段应为 None
            })
        );

        // 3. 测试 Delegated (Stake) 状态
        let voter_pubkey = solana_pubkey::new_rand(); // 随机生成一个验证者投票账户公钥
        let stake_details = Stake { // 创建质押详情
            delegation: Delegation {
                voter_pubkey,
                stake: 20,                     // 委托 20 lamports
                activation_epoch: 2,         // 激活于 epoch 2
                deactivation_epoch: u64::MAX, // 尚未停用 (u64::MAX 表示“永不”或“遥远的未来”)
                warmup_cooldown_rate: 0.25,    // 废弃字段
            },
            credits_observed: 10, // 观察到的信用点数
        };

        // 使用相同的元数据 `meta_data` 和新的 `stake_details` 创建 Delegated 状态
        // `StakeFlags::empty()` 表示没有设置额外的标志位
        let stake_state_delegated = StakeStateV2::Stake(meta_data, stake_details, StakeFlags::empty());
        let stake_data_delegated = serialize(&stake_state_delegated).unwrap();
        assert_eq!(
            parse_stake(&stake_data_delegated).unwrap(),
            StakeAccountType::Delegated(UiStakeAccount { // 预期的 Delegated 状态
                meta: UiMeta { // 元数据与 Initialized 状态时相同
                    rent_exempt_reserve: 42.to_string(),
                    authorized: UiAuthorized {
                        staker: staker_withdrawer_pubkey.to_string(),
                        withdrawer: staker_withdrawer_pubkey.to_string(),
                    },
                    lockup: UiLockup {
                        unix_timestamp: 0,
                        epoch: 1,
                        custodian: custodian_pubkey.to_string(),
                    }
                },
                stake: Some(UiStake { // 质押详情，包装在 Some 中
                    delegation: UiDelegation {
                        voter: voter_pubkey.to_string(),
                        stake: 20.to_string(),
                        activation_epoch: 2.to_string(),
                        deactivation_epoch: u64::MAX.to_string(),
                        warmup_cooldown_rate: 0.25,
                    },
                    credits_observed: 10,
                })
            })
        );

        // 4. 测试 RewardsPool 状态
        let stake_state_rewards_pool = StakeStateV2::RewardsPool;
        let stake_data_rewards_pool = serialize(&stake_state_rewards_pool).unwrap();
        assert_eq!(
            parse_stake(&stake_data_rewards_pool).unwrap(),
            StakeAccountType::RewardsPool
        );

        // 5. 测试无效数据的情况
        let bad_data = vec![1, 2, 3, 4]; // 创建一些明显不是有效质押账户数据的字节
        // 尝试用这些坏数据解析，并断言结果是 `Err`
        assert!(parse_stake(&bad_data).is_err());
    }
}
