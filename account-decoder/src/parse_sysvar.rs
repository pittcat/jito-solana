// 文件功能说明：
// 这个文件负责解析 Solana 网络中的“系统变量”（System Variables，简称 Sysvars）账户。
// Sysvars 是一组特殊的、由系统维护的账户，它们提供了关于集群状态、时间、配置参数等动态信息的只读访问。
// Solana 程序（即智能合约）可以直接在链上读取这些 Sysvar 账户的数据，以便在执行时获取必要的上下文信息，
// 例如当前的 slot（槽，Solana 中基本的时间和区块单位）、epoch（时期）、最近的区块哈希、租金参数等。
// 这使得程序能够根据网络状态做出相应的行为，而无需依赖外部数据源。
//
// 每种 Sysvar 都有一个固定且众所周知的公钥（地址）。本文件中的代码通过匹配传入的账户公钥，
// 来识别它属于哪种 Sysvar，然后将其二进制数据反序列化为相应的 Rust 结构体，
// 并最终转换为更适合用户界面（UI）展示的结构化数据。
//
// 例如，`Clock` Sysvar 提供了当前的时间信息，`Rent` Sysvar 提供了账户租金的计算参数。
// 开发者在编写 Solana 程序时，经常需要访问这些 Sysvar 来确保其程序的正确性和效率。
// 此模块的目标就是提供一个统一的接口来解析所有不同类型的 Sysvar 数据。

// `#[allow(deprecated)]` 属性用于允许在此文件中使用一些已被标记为“不推荐使用”（deprecated）的 Sysvar 类型，
// 例如 `Fees` 和 `RecentBlockhashes`（虽然 `RecentBlockhashes` 的概念仍然存在，但其具体实现和访问方式可能已更新，
// 例如通过 `RecentSlothashes`）。这是为了保持对旧版本数据的兼容性或逐步迁移。
#[allow(deprecated)]
use solana_sysvar::{fees::Fees, recent_blockhashes::RecentBlockhashes}; // 导入已废弃的 Fees 和 RecentBlockhashes Sysvar 原始结构。
use {
    // 从当前 crate (solana-account-decoder) 导入所需类型。
    crate::{
        parse_account_data::{ParsableAccount, ParseAccountError}, // `ParsableAccount` 用于标识账户类型，`ParseAccountError` 定义解析错误。
        StringAmount, UiFeeCalculator, // `StringAmount` 用于表示金额的字符串，`UiFeeCalculator` 是UI友好的费用计算器。
    },
    bincode::deserialize, // 导入 `bincode` 库的 `deserialize` 函数，用于将二进制数据转换为 Rust 结构体。
    bv::BitVec,           // 导入 `BitVec` 类型，用于表示位向量，常见于 `SlotHistory` Sysvar。
    // 从 Solana 时钟库导入时间相关的基本类型。
    solana_clock::{Clock, Epoch, Slot, UnixTimestamp}, // `Clock` 是时钟Sysvar的原始结构，`Epoch`、`Slot`、`UnixTimestamp` 是时间单位。
                                                      // Slot (槽): Solana 中最小的时间单位，约400毫秒，每个槽理论上可以产生一个区块。
                                                      // Epoch (时期): 由多个槽组成，是网络中某些参数（如验证者列表、通胀率）更新的周期。
                                                      // UnixTimestamp: 标准的Unix时间戳。
    solana_epoch_schedule::EpochSchedule, // `EpochSchedule` Sysvar 的原始结构，描述 Epoch 的调度规则。
    solana_pubkey::Pubkey,                // 导入 `Pubkey` 类型，表示 Solana 账户的公钥（地址）。
    solana_rent::Rent,                    // `Rent` Sysvar 的原始结构，描述账户租金规则。
    solana_sdk_ids::sysvar,               // 导入 `sysvar` 模块，其中包含了所有 Sysvar 的固定公钥ID。
    solana_slot_hashes::SlotHashes,       // `SlotHashes` Sysvar 的原始结构，存储最近的 slot 哈希。
    solana_slot_history::{self as slot_history, SlotHistory}, // `SlotHistory` Sysvar 的原始结构，记录 slot 的历史状态。
                                                              // `self as slot_history` 是为了方便地引用 `slot_history::MAX_ENTRIES` 等常量。
    // 从 Solana Sysvar 集合库中导入其他 Sysvar 的原始结构。
    solana_sysvar::{
        epoch_rewards::EpochRewards,             // `EpochRewards` Sysvar，记录一个 Epoch 内的奖励分配情况。
        last_restart_slot::LastRestartSlot,    // `LastRestartSlot` Sysvar，记录集群最后一次重启的 slot。
        rewards::Rewards,                        // `Rewards` Sysvar (通常与 EpochRewards 一起看)，记录奖励相关的点值。
        stake_history::{StakeHistory, StakeHistoryEntry}, // `StakeHistory` Sysvar，记录每个 Epoch 的质押活动摘要。
    },
};

// 函数 `parse_sysvar`：
// 功能：根据传入的 Sysvar 账户公钥，解析其原始二进制数据。
// 参数：
//   - `data`: `&[u8]`，一个字节切片，代表 Sysvar 账户的原始数据。
//   - `pubkey`: `&Pubkey`，被解析的 Sysvar 账户的公钥。通过这个公钥来判断是哪种 Sysvar。
// 返回值：
//   - `Result<SysvarAccountType, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(SysvarAccountType)`，其中 `SysvarAccountType` 是一个枚举，
//       包含了具体 Sysvar 类型及其用户友好的数据结构。
//     - 如果公钥不对应任何已知的 Sysvar，或者数据反序列化失败，返回 `Err(ParseAccountError)`。
// 设计思路：
//   1. 使用一个大的 `if/else if` 链来判断 `pubkey` 属于哪个已知的 Sysvar ID (例如 `sysvar::clock::id()`)。
//   2. 对于每种匹配到的 Sysvar 类型：
//      a. 调用 `bincode::deserialize::<OriginalStructType>(data)` 将原始字节数据反序列化为其对应的链上原始结构体类型
//         (例如，对于 Clock Sysvar，反序列化为 `solana_clock::Clock`)。
//      b. `.ok()` 将 `Result` 转换为 `Option`，如果反序列化失败则为 `None`。
//      c. `.map(|original_struct| ...)`：如果反序列化成功（得到 `Some(original_struct)`），则执行闭包内的转换逻辑：
//         i.  对于大部分 Sysvar，会调用 `.into()` 将原始结构体转换为其对应的用户友好（UI）结构体
//             (例如，`Clock` 转为 `UiClock`)。这要求 UI 结构体实现了 `From<OriginalStructType>` 特性。
//         ii. 对于某些包含列表的 Sysvar（如 `RecentBlockhashes`, `SlotHashes`, `StakeHistory`），
//             需要额外遍历列表中的每个条目，并将每个条目转换为其 UI 版本，然后收集到一个新的 Vec 中。
//         iii. 将转换后的 UI 结构体包装在相应的 `SysvarAccountType` 枚举成员中 (例如 `SysvarAccountType::Clock(ui_clock)`)。
//   3. 如果 `pubkey` 不匹配任何已知的 Sysvar ID，则整个 `if/else if` 链的结果是 `None`。
//   4. `#[allow(deprecated)]` 用在整个 `parsed_account` 块上，是因为内部的某些分支（如 `Fees` 和 `RecentBlockhashes`）
//      处理的是已废弃的 Sysvar 类型。
//   5. 最后，`parsed_account` 是一个 `Option<SysvarAccountType>`。
//      - 如果它是 `Some(account_type)`，表示解析成功，返回 `Ok(account_type)`。
//      - 如果它是 `None`（表示无法识别 Sysvar 类型或反序列化失败），则返回
//        `Err(ParseAccountError::AccountNotParsable(ParsableAccount::Sysvar))`，
//        指明这是一个 Sysvar 类型的账户，但无法解析。
pub fn parse_sysvar(data: &[u8], pubkey: &Pubkey) -> Result<SysvarAccountType, ParseAccountError> {
    #[allow(deprecated)] // 允许在块内部使用已废弃的类型
    let parsed_account = {
        // 通过匹配公钥来确定 Sysvar 类型并进行相应的解析
        if pubkey == &sysvar::clock::id() { // Clock Sysvar (时钟)
            deserialize::<Clock>(data) // 反序列化为原始 Clock 结构
                .ok() // Result -> Option
                .map(|clock| SysvarAccountType::Clock(clock.into())) // 转换为 UiClock 并包装
        } else if pubkey == &sysvar::epoch_schedule::id() { // EpochSchedule Sysvar (Epoch 调度)
            // EpochSchedule 结构本身就适合UI展示，所以直接使用，不转换为特定的 UiEpochSchedule
            deserialize(data).ok().map(SysvarAccountType::EpochSchedule)
        } else if pubkey == &sysvar::fees::id() { // Fees Sysvar (交易费用，已废弃)
            deserialize::<Fees>(data)
                .ok()
                .map(|fees| SysvarAccountType::Fees(fees.into())) // 转换为 UiFees
        } else if pubkey == &sysvar::recent_blockhashes::id() { // RecentBlockhashes Sysvar (最近区块哈希，也称 Slot 哈希，名称有点历史遗留)
            deserialize::<RecentBlockhashes>(data)
                .ok()
                .map(|recent_blockhashes_iter| { // 注意：RecentBlockhashes 实现了 IntoIterator
                    let ui_entries = recent_blockhashes_iter
                        .iter() // 遍历每个条目
                        .map(|entry| UiRecentBlockhashesEntry { // 将每个条目转换为 UiRecentBlockhashesEntry
                            blockhash: entry.blockhash.to_string(), // 哈希转字符串
                            fee_calculator: entry.fee_calculator.into(), // FeeCalculator 转 UiFeeCalculator
                        })
                        .collect(); // 收集成 Vec<UiRecentBlockhashesEntry>
                    SysvarAccountType::RecentBlockhashes(ui_entries)
                })
        } else if pubkey == &sysvar::rent::id() { // Rent Sysvar (租金)
            deserialize::<Rent>(data)
                .ok()
                .map(|rent| SysvarAccountType::Rent(rent.into())) // 转换为 UiRent
        } else if pubkey == &sysvar::rewards::id() { // Rewards Sysvar (奖励)
            deserialize::<Rewards>(data)
                .ok()
                .map(|rewards| SysvarAccountType::Rewards(rewards.into())) // 转换为 UiRewards
        } else if pubkey == &sysvar::slot_hashes::id() { // SlotHashes Sysvar (Slot 哈希列表)
            deserialize::<SlotHashes>(data).ok().map(|slot_hashes_iter| { // SlotHashes 也实现了 IntoIterator
                let ui_entries = slot_hashes_iter
                    .iter()
                    .map(|slot_hash_tuple| UiSlotHashEntry { // 元组 (Slot, Hash) 转 UiSlotHashEntry
                        slot: slot_hash_tuple.0,
                        hash: slot_hash_tuple.1.to_string(),
                    })
                    .collect();
                SysvarAccountType::SlotHashes(ui_entries)
            })
        } else if pubkey == &sysvar::slot_history::id() { // SlotHistory Sysvar (Slot 历史)
            deserialize::<SlotHistory>(data).ok().map(|slot_history| {
                SysvarAccountType::SlotHistory(UiSlotHistory {
                    next_slot: slot_history.next_slot,
                    // SlotHistory 的 `bits` 字段是一个 BitVec，需要特殊格式化为01字符串
                    bits: format!("{:?}", SlotHistoryBits(slot_history.bits)),
                })
            })
        } else if pubkey == &sysvar::stake_history::id() { // StakeHistory Sysvar (质押历史)
            deserialize::<StakeHistory>(data).ok().map(|stake_history_iter| { // StakeHistory 实现了 IntoIterator
                let ui_entries = stake_history_iter
                    .iter()
                    .map(|entry_tuple| UiStakeHistoryEntry { // 元组 (Epoch, StakeHistoryEntry) 转 UiStakeHistoryEntry
                        epoch: entry_tuple.0,
                        stake_history: entry_tuple.1.clone(), // StakeHistoryEntry 直接使用 (需要 clone)
                    })
                    .collect();
                SysvarAccountType::StakeHistory(ui_entries)
            })
        } else if pubkey == &sysvar::last_restart_slot::id() { // LastRestartSlot Sysvar (最后重启 Slot)
            deserialize::<LastRestartSlot>(data)
                .ok()
                .map(|last_restart_slot_struct| {
                    // LastRestartSlot 结构体内部只有一个字段 last_restart_slot
                    SysvarAccountType::LastRestartSlot(UiLastRestartSlot {
                        last_restart_slot: last_restart_slot_struct.last_restart_slot,
                    })
                })
        } else if pubkey == &sysvar::epoch_rewards::id() { // EpochRewards Sysvar (Epoch 奖励)
            deserialize::<EpochRewards>(data)
                .ok()
                .map(|epoch_rewards| SysvarAccountType::EpochRewards(epoch_rewards.into())) // 转换为 UiEpochRewards
        } else {
            // 如果公钥不匹配任何已知的 Sysvar ID，则返回 None
            None
        }
    };
    // 将 Option<SysvarAccountType> 转换为 Result<SysvarAccountType, ParseAccountError>
    // 如果是 None，则返回指定的错误。
    parsed_account.ok_or(ParseAccountError::AccountNotParsable(
        ParsableAccount::Sysvar, // 指明是 Sysvar 类型的账户解析失败
    ))
}

// 枚举 `SysvarAccountType`:
// 表示已解析的 Sysvar 账户的具体类型，并包含其用户友好的数据结构。
// `#[derive(Debug, Serialize, Deserialize, PartialEq)]`:
//   - `Debug`: 允许调试打印。
//   - `Serialize`, `Deserialize`: 允许 `serde` 进行序列化和反序列化。
//   - `PartialEq`: 允许比较实例是否相等（主要用于测试）。
// `#[serde(rename_all = "camelCase", tag = "type", content = "info")]`:
//   - `rename_all = "camelCase"`: JSON 字段和枚举成员名使用驼峰式。
//   - `tag = "type"`: JSON 中会有一个 "type" 字段，值为枚举成员名 (e.g., "clock", "epochSchedule")。
//   - `content = "info"`: 枚举成员关联的数据会放在 "info" 字段中 (如果成员有数据)。
//     对于像 `EpochSchedule` 这样直接使用原始结构的成员，其内容会直接在 "info" 字段下。
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum SysvarAccountType {
    Clock(UiClock),                             // 时钟信息
    EpochSchedule(EpochSchedule),               // Epoch 调度信息 (直接使用原始结构)
    #[allow(deprecated)]                        // 允许使用已废弃的 Fees 类型
    Fees(UiFees),                               // 交易费用信息 (已废弃)
    #[allow(deprecated)]                        // 允许使用已废弃的 RecentBlockhashes 类型
    RecentBlockhashes(Vec<UiRecentBlockhashesEntry>), // 最近区块哈希列表 (名称有历史原因)
    Rent(UiRent),                               // 租金信息
    Rewards(UiRewards),                         // 奖励信息
    SlotHashes(Vec<UiSlotHashEntry>),           // Slot 哈希列表
    SlotHistory(UiSlotHistory),                 // Slot 历史记录
    StakeHistory(Vec<UiStakeHistoryEntry>),     // 质押历史记录
    LastRestartSlot(UiLastRestartSlot),         // 最后重启 Slot 信息
    EpochRewards(UiEpochRewards),               // Epoch 奖励分配信息
}

// 结构体 `UiClock`:
// 用户友好的时钟 Sysvar 数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]`:
//   - `Eq`: `PartialEq`的增强，表示全等。
//   - `Default`: 允许创建此结构体的默认实例。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiClock {
    pub slot: Slot,                         // 当前的 Slot (槽) 号。
    pub epoch: Epoch,                       // 当前的 Epoch (时期) 号。
    pub epoch_start_timestamp: UnixTimestamp, // 当前 Epoch 开始时的 Unix 时间戳。
    pub leader_schedule_epoch: Epoch,       // 当前用于计算领导者调度表的 Epoch。可能与 `epoch` 不同。
                                            // 领导者调度表决定了在特定 Slot 由哪个验证者出块。
    pub unix_timestamp: UnixTimestamp,      // 当前的 Unix 时间戳。
}

// 实现 `From<Clock> for UiClock`:
// 定义如何从原始的链上 `Clock` 结构体转换为用户友好的 `UiClock`。
impl From<Clock> for UiClock {
    fn from(clock: Clock) -> Self {
        Self { // 直接复制所有字段，因为它们的类型和含义在 UI 版本中保持一致。
            slot: clock.slot,
            epoch: clock.epoch,
            epoch_start_timestamp: clock.epoch_start_timestamp,
            leader_schedule_epoch: clock.leader_schedule_epoch,
            unix_timestamp: clock.unix_timestamp,
        }
    }
}

// 结构体 `UiFees`: (已废弃)
// 用户友好的交易费用 Sysvar 数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiFees {
    // `fee_calculator`: 包含具体的费用计算规则，使用 `UiFeeCalculator` 结构。
    pub fee_calculator: UiFeeCalculator,
}
// `#[allow(deprecated)]` 因为 `Fees` 本身是废弃的。
#[allow(deprecated)]
impl From<Fees> for UiFees {
    fn from(fees: Fees) -> Self {
        Self {
            fee_calculator: fees.fee_calculator.into(), // 原始 FeeCalculator 转 UiFeeCalculator
        }
    }
}

// 结构体 `UiRent`:
// 用户友好的租金 Sysvar 数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiRent {
    // `lamports_per_byte_year`: 存储每字节数据一年所需的租金（单位：lamports），字符串表示。
    pub lamports_per_byte_year: StringAmount,
    // `exemption_threshold`: 账户余额需要达到多少倍的年租金才能免除租金。
    // 例如，如果值为 2.0，表示账户余额至少是其两年租金的总和，才能免租。
    pub exemption_threshold: f64,
    // `burn_percent`: 从租金收入中销毁的百分比 (0-100)。
    // Solana 的一个经济机制，部分租金会被销毁，从而减少 SOL 的总供应量。
    pub burn_percent: u8,
}

// 实现 `From<Rent> for UiRent`:
// 定义如何从原始的链上 `Rent` 结构体转换为用户友好的 `UiRent`。
impl From<Rent> for UiRent {
    fn from(rent: Rent) -> Self {
        Self {
            lamports_per_byte_year: rent.lamports_per_byte_year.to_string(), // u64 转 String
            exemption_threshold: rent.exemption_threshold, // f64 直接复制
            burn_percent: rent.burn_percent,             // u8 直接复制
        }
    }
}

// 结构体 `UiRewards`:
// 用户友好的奖励 Sysvar 数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiRewards {
    // `validator_point_value`: 每个验证者“点数”对应的奖励值。
    // 验证者通过参与共识（投票、出块）来获得点数，这个值决定了这些点数能兑换多少奖励。
    pub validator_point_value: f64,
}

// 实现 `From<Rewards> for UiRewards`:
// 定义如何从原始的链上 `Rewards` 结构体转换为用户友好的 `UiRewards`。
impl From<Rewards> for UiRewards {
    fn from(rewards: Rewards) -> Self {
        Self {
            validator_point_value: rewards.validator_point_value, // f64 直接复制
        }
    }
}

// 结构体 `UiRecentBlockhashesEntry`: (应为 SlotHashes 条目)
// 用户友好的最近区块哈希列表中的单个条目表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiRecentBlockhashesEntry {
    // `blockhash`: 区块哈希（实际是 Slot 哈希）的字符串表示。
    pub blockhash: String,
    // `fee_calculator`: 与此哈希关联的交易费用计算器。
    pub fee_calculator: UiFeeCalculator,
}

// 结构体 `UiSlotHashEntry`:
// 用户友好的 Slot 哈希列表中的单个条目表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiSlotHashEntry {
    pub slot: Slot,   // Slot 号码。
    pub hash: String, // 该 Slot 对应的哈希值的字符串表示。
}

// 结构体 `UiSlotHistory`:
// 用户友好的 Slot 历史 Sysvar 数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiSlotHistory {
    // `next_slot`: `SlotHistory` 内部循环缓冲区中下一个将要记录的 Slot 号。
    pub next_slot: Slot,
    // `bits`: 一个表示最近 Slot 是否存在的位图（bitmap）的字符串形式。
    // 通常是一个由 '0' 和 '1' 组成的字符串，每个位对应一个 Slot，'1' 表示存在，'0' 表示不存在或未记录。
    pub bits: String,
}

// 辅助结构体 `SlotHistoryBits`，用于自定义 `SlotHistory` 中 `BitVec` 的调试输出格式。
// 它包装了一个 `BitVec<u64>`。
struct SlotHistoryBits(BitVec<u64>);

// 为 `SlotHistoryBits` 实现 `std::fmt::Debug` 特性。
// Rust 特有概念：
//   - `impl std::fmt::Debug for TypeName`: 这是为 `TypeName` 实现 `Debug` 特性的标准方式。
//     `Debug` 特性允许我们使用 `{:?}` 格式化操作符来打印一个类型的实例，这对于调试非常有用。
//     通过自定义实现，我们可以控制打印的格式。
//   - `fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result`: 这是 `Debug` 特性要求实现的方法。
//     `&self` 表示对当前实例的不可变借用。
//     `f` 是一个格式化器 (`Formatter`)，用于写入输出字符串。
//     返回 `std::fmt::Result`，如果写入成功则为 `Ok(())`，否则为 `Err`。
impl std::fmt::Debug for SlotHistoryBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `slot_history::MAX_ENTRIES` 是 `SlotHistory` Sysvar能记录的最大条目数。
        // 遍历所有可能的条目索引。
        for i in 0..slot_history::MAX_ENTRIES {
            if self.0.get(i) { // `self.0` 访问内部的 `BitVec`。`.get(i)` 检查第 `i` 位是否为1。
                write!(f, "1")?; // 如果是1，写入 "1"。`?` 操作符用于错误传播：如果 `write!` 返回 `Err`，则 `fmt` 函数立即返回 `Err`。
            } else {
                write!(f, "0")?; // 否则写入 "0"。
            }
        }
        Ok(()) // 所有位都成功写入，返回 Ok。
    }
}

// 结构体 `UiStakeHistoryEntry`:
// 用户友好的质押历史 Sysvar 中的单个条目表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiStakeHistoryEntry {
    pub epoch: Epoch,                         // 该历史条目对应的 Epoch (时期) 号。
    // `stake_history`: 包含该 Epoch 质押统计信息的 `StakeHistoryEntry` 结构。
    // `StakeHistoryEntry` 结构本身就比较简单，直接在 UI 版本中使用，无需进一步转换。
    pub stake_history: StakeHistoryEntry,
}

// 结构体 `UiLastRestartSlot`:
// 用户友好的最后重启 Slot Sysvar 数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiLastRestartSlot {
    // `last_restart_slot`: 集群最后一次重启的 Slot 号。
    pub last_restart_slot: Slot,
}

// 结构体 `UiEpochRewards`:
// 用户友好的 Epoch 奖励 Sysvar 数据表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiEpochRewards {
    // `distribution_starting_block_height`: 开始进行此 Epoch 奖励分配的区块高度。
    pub distribution_starting_block_height: u64,
    // `num_partitions`: （内部使用）奖励分配过程可能被分成的分区数量。
    pub num_partitions: u64,
    // `parent_blockhash`: 此 Epoch 奖励计算所依据的父区块的哈希，字符串表示。
    pub parent_blockhash: String,
    // `total_points`: 此 Epoch 中所有参与验证者累积的总点数，字符串表示。
    pub total_points: String,
    // `total_rewards`: 此 Epoch 中可供分配的总奖励金额（单位：lamports），字符串表示。
    pub total_rewards: String,
    // `distributed_rewards`: 此 Epoch 中已经分配出去的奖励金额（单位：lamports），字符串表示。
    pub distributed_rewards: String,
    // `active`: 一个布尔值，指示此 Epoch 的奖励分配过程是否仍在进行中。
    pub active: bool,
}

// 实现 `From<EpochRewards> for UiEpochRewards`:
// 定义如何从原始的链上 `EpochRewards` 结构体转换为用户友好的 `UiEpochRewards`。
impl From<EpochRewards> for UiEpochRewards {
    fn from(epoch_rewards: EpochRewards) -> Self {
        Self {
            distribution_starting_block_height: epoch_rewards.distribution_starting_block_height,
            num_partitions: epoch_rewards.num_partitions,
            parent_blockhash: epoch_rewards.parent_blockhash.to_string(), // Pubkey/Hash 转 String
            total_points: epoch_rewards.total_points.to_string(),       // u128 转 String
            total_rewards: epoch_rewards.total_rewards.to_string(),     // u64 转 String
            distributed_rewards: epoch_rewards.distributed_rewards.to_string(), // u64 转 String
            active: epoch_rewards.active,
        }
    }
}

// 测试模块
#[cfg(test)]
mod test {
    // `#[allow(deprecated)]` 允许在测试中使用已废弃的 `IterItem`。
    #[allow(deprecated)]
    use solana_sysvar::recent_blockhashes::IterItem; // `IterItem` 是 `RecentBlockhashes` 迭代器产生的元素类型。
    // 导入父模块的所有项 (`super::*`) 和测试中需要的特定项。
    use {
        super::*, solana_account::create_account_for_test, solana_fee_calculator::FeeCalculator,
        solana_hash::Hash,
    };
    // `create_account_for_test` 是一个测试辅助函数，用于创建一个包含特定数据的模拟账户。
    // `FeeCalculator` 用于创建费用计算器实例。
    // `Hash` 用于创建哈希实例。

    // 测试函数 `test_parse_sysvars`
    // `#[test]` 宏标记这是一个单元测试函数。
    #[test]
    fn test_parse_sysvars() {
        // 创建一个用于测试的示例哈希。
        let hash = Hash::new_from_array([1; 32]); // 创建一个所有字节都为1的32字节哈希。

        // 1. 测试 Clock Sysvar
        let clock_sysvar_account = create_account_for_test(&Clock::default()); // 创建一个包含默认 Clock 数据的账户。
        assert_eq!( // 断言解析结果是否符合预期。
            parse_sysvar(&clock_sysvar_account.data, &sysvar::clock::id()).unwrap(),
            SysvarAccountType::Clock(UiClock::default()), // 预期得到默认的 UiClock。
        );

        // 2. 测试 EpochSchedule Sysvar
        let epoch_schedule_data = EpochSchedule { // 创建一个自定义的 EpochSchedule 实例。
            slots_per_epoch: 12,
            leader_schedule_slot_offset: 0,
            warmup: false,
            first_normal_epoch: 1,
            first_normal_slot: 12,
        };
        let epoch_schedule_sysvar_account = create_account_for_test(&epoch_schedule_data);
        assert_eq!(
            parse_sysvar(&epoch_schedule_sysvar_account.data, &sysvar::epoch_schedule::id()).unwrap(),
            SysvarAccountType::EpochSchedule(epoch_schedule_data), // EpochSchedule 直接使用原始结构。
        );

        // 3. 测试已废弃的 Sysvars (Fees 和 RecentBlockhashes)
        // `#[allow(deprecated)]` 允许在这个块中使用废弃的类型。
        #[allow(deprecated)]
        {
            // 3a. 测试 Fees Sysvar
            let fees_sysvar_account = create_account_for_test(&Fees::default());
            assert_eq!(
                parse_sysvar(&fees_sysvar_account.data, &sysvar::fees::id()).unwrap(),
                SysvarAccountType::Fees(UiFees::default()), // 预期得到默认的 UiFees。
            );

            // 3b. 测试 RecentBlockhashes Sysvar
            // 创建一个包含一个条目的 RecentBlockhashes 实例。
            // `IterItem(0, &hash, 10)` 表示第0个条目，区块哈希为 `hash`，关联的费用lamports_per_signature为10。
            let recent_blockhashes_data: RecentBlockhashes =
                vec![IterItem(0, &hash, 10)].into_iter().collect();
            let recent_blockhashes_sysvar_account = create_account_for_test(&recent_blockhashes_data);
            assert_eq!(
                parse_sysvar(
                    &recent_blockhashes_sysvar_account.data,
                    &sysvar::recent_blockhashes::id()
                )
                .unwrap(),
                SysvarAccountType::RecentBlockhashes(vec![UiRecentBlockhashesEntry { // 预期得到包含一个转换后条目的列表。
                    blockhash: hash.to_string(),
                    fee_calculator: FeeCalculator::new(10).into(), // 费用计算器也转换为UI版本。
                }]),
            );
        }

        // 4. 测试 Rent Sysvar
        let rent_data = Rent { // 创建自定义 Rent 实例。
            lamports_per_byte_year: 10,
            exemption_threshold: 2.0,
            burn_percent: 5,
        };
        let rent_sysvar_account = create_account_for_test(&rent_data);
        assert_eq!(
            parse_sysvar(&rent_sysvar_account.data, &sysvar::rent::id()).unwrap(),
            SysvarAccountType::Rent(rent_data.into()), // 转换为 UiRent。
        );

        // 5. 测试 Rewards Sysvar
        let rewards_sysvar_account = create_account_for_test(&Rewards::default());
        assert_eq!(
            parse_sysvar(&rewards_sysvar_account.data, &sysvar::rewards::id()).unwrap(),
            SysvarAccountType::Rewards(UiRewards::default()), // 预期得到默认的 UiRewards。
        );

        // 6. 测试 SlotHashes Sysvar
        let mut slot_hashes_data = SlotHashes::default();
        slot_hashes_data.add(1, hash); // 添加一个条目：slot 1，哈希为 `hash`。
        let slot_hashes_sysvar_account = create_account_for_test(&slot_hashes_data);
        assert_eq!(
            parse_sysvar(&slot_hashes_sysvar_account.data, &sysvar::slot_hashes::id()).unwrap(),
            SysvarAccountType::SlotHashes(vec![UiSlotHashEntry { // 预期得到包含一个转换后条目的列表。
                slot: 1,
                hash: hash.to_string(),
            }]),
        );

        // 7. 测试 SlotHistory Sysvar
        let mut slot_history_data = SlotHistory::default();
        slot_history_data.add(42); // 在 SlotHistory 中标记 slot 42 为存在。
        let slot_history_sysvar_account = create_account_for_test(&slot_history_data);
        assert_eq!(
            parse_sysvar(&slot_history_sysvar_account.data, &sysvar::slot_history::id()).unwrap(),
            SysvarAccountType::SlotHistory(UiSlotHistory { // 预期得到转换后的 UiSlotHistory。
                next_slot: slot_history_data.next_slot,
                bits: format!("{:?}", SlotHistoryBits(slot_history_data.bits)), // bits 字段通过自定义 Debug 实现格式化。
            }),
        );

        // 8. 测试 StakeHistory Sysvar
        let mut stake_history_data = StakeHistory::default();
        let stake_history_entry_data = StakeHistoryEntry { // 创建一个示例 StakeHistoryEntry。
            effective: 10,    // 有效质押
            activating: 2,    // 正在激活的质押
            deactivating: 3,  // 正在停用的质押
        };
        stake_history_data.add(1, stake_history_entry_data.clone()); // 在 epoch 1 添加此条目。
        let stake_history_sysvar_account = create_account_for_test(&stake_history_data);
        assert_eq!(
            parse_sysvar(&stake_history_sysvar_account.data, &sysvar::stake_history::id()).unwrap(),
            SysvarAccountType::StakeHistory(vec![UiStakeHistoryEntry { // 预期得到包含一个转换后条目的列表。
                epoch: 1,
                stake_history: stake_history_entry_data, // StakeHistoryEntry 直接使用。
            }]),
        );

        // 9. 测试无效公钥和无效数据的情况
        let bad_pubkey = solana_pubkey::new_rand(); // 创建一个随机的、不对应任何已知 Sysvar 的公钥。
        // 使用有效的 StakeHistory 数据但传入错误的公钥，预期解析失败。
        assert!(parse_sysvar(&stake_history_sysvar_account.data, &bad_pubkey).is_err());

        let bad_data = vec![0; 4]; // 创建一些明显无效的账户数据。
        // 使用有效的 StakeHistory Sysvar ID 但传入坏数据，预期解析失败。
        assert!(parse_sysvar(&bad_data, &sysvar::stake_history::id()).is_err());

        // 10. 测试 LastRestartSlot Sysvar
        let last_restart_slot_data = LastRestartSlot {
            last_restart_slot: 1282,
        };
        let last_restart_slot_account = create_account_for_test(&last_restart_slot_data);
        assert_eq!(
            parse_sysvar(
                &last_restart_slot_account.data,
                &sysvar::last_restart_slot::id()
            )
            .unwrap(),
            SysvarAccountType::LastRestartSlot(UiLastRestartSlot { // 预期得到转换后的 UiLastRestartSlot。
                last_restart_slot: 1282
            })
        );

        // 11. 测试 EpochRewards Sysvar
        let epoch_rewards_data = EpochRewards { // 创建自定义 EpochRewards 实例。
            distribution_starting_block_height: 42,
            total_rewards: 100,
            distributed_rewards: 20,
            active: true,
            ..EpochRewards::default() // 其余字段使用默认值。
        };
        let epoch_rewards_sysvar_account = create_account_for_test(&epoch_rewards_data);
        assert_eq!(
            parse_sysvar(&epoch_rewards_sysvar_account.data, &sysvar::epoch_rewards::id()).unwrap(),
            SysvarAccountType::EpochRewards(epoch_rewards_data.into()), // 转换为 UiEpochRewards。
        );
    }
}
