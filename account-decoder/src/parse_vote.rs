// 文件功能说明：
// 这个文件专门负责解析 Solana 网络中的“投票账户”（Vote Account）数据。
// 投票账户是 Solana 共识机制的核心组成部分，每个验证者（Validator）都拥有一个投票账户。
// 验证者通过其投票账户执行以下关键操作：
// 1. 投票 (Voting): 当验证者验证了领导者（当前负责出块的验证者）产生的新区块后，
//    会广播一个投票事务，表明其对该区块有效性的认可。这些投票是网络达成共识的基础。
// 2. 身份标识: 投票账户的公钥是验证者在网络上的主要身份标识之一。
// 3. 接收质押和奖励: 用户将 SOL 质押给验证者的投票账户，以支持该验证者并分享奖励。
//
// 投票账户（在链上以 `VoteState` 结构存储）包含了验证者状态和活动的大量信息，例如：
// - 验证者节点的身份公钥 (`node_pubkey`)。
// - 有权提取此投票账户资金的地址 (`authorized_withdrawer`)。
// - 验证者收取的佣金比例 (`commission`)。
// - 最近的投票记录 (`votes`)，每个投票记录（称为 `Lockout`）包含投票的 Slot 和确认数。
// - 验证者当前认为的“根” Slot (`root_slot`)，即已被网络高度确认的区块。
// - 不同 Epoch（时期）的授权投票者 (`authorized_voters`)，允许验证者轮换投票密钥。
// - 历史授权投票者信息 (`prior_voters`)。
// - 在各个 Epoch 中获得的“信用点数” (`epoch_credits`)，用于计算奖励。
// - 最后一次投票的时间戳 (`last_timestamp`)，用于判断验证者活跃度。
//
// 本文件中的代码会将投票账户的原始二进制数据反序列化，并将其转换为用户友好的、
// 结构化的 Rust 类型（主要是 `UiVoteState` 及其子结构），方便钱包、浏览器、
// 质押监控工具等向用户展示验证者的详细信息和投票活动。

use {
    // 从当前 crate (solana-account-decoder) 导入解析错误类型和用于金额的字符串类型。
    crate::{parse_account_data::ParseAccountError, StringAmount},
    // 从 Solana 时钟库导入 Epoch (时期) 和 Slot (槽) 类型。
    solana_clock::{Epoch, Slot},
    // 导入公钥类型。
    solana_pubkey::Pubkey,
    // 从 Solana 投票程序接口库导入投票账户相关的状态定义。
    solana_vote_interface::state::{BlockTimestamp, Lockout, VoteState},
    // `BlockTimestamp`: 包含 slot 和 unix_timestamp 的结构，用于记录区块或投票的时间。
    // `Lockout`: 表示一次投票及其获得的确认数。
    // `VoteState`: 投票账户在链上存储的原始状态结构。
};

// 函数 `parse_vote`：
// 功能：解析投票账户的原始二进制数据。
// 参数：
//   - `data`: `&[u8]`，一个字节切片，代表投票账户的原始数据。
// 返回值：
//   - `Result<VoteAccountType, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(VoteAccountType)`，其中 `VoteAccountType` 枚举目前只有一个成员 `Vote`，
//       它包装了用户友好的 `UiVoteState` 结构。
//     - 如果反序列化失败，返回 `Err(ParseAccountError)`。
// 设计思路：
//   1. 尝试使用 `VoteState::deserialize(data)` 将传入的原始字节数据反序列化为 `VoteState` 结构。
//      `VoteState` 是链上投票账户状态的原始表示。
//      如果反序列化失败，通过 `.map_err(ParseAccountError::from)` 将底层的错误（通常是 `bincode` 错误或指令错误）
//      转换为 `ParseAccountError` 并返回。`?` 操作符用于错误传播。
//      注意：这里的 `mut vote_state` 是因为后续调用 `vote_state.prior_voters().buf()` 可能需要可变借用（取决于具体实现，但通常迭代器方法可能需要）。
//   2. 如果反序列化成功，则从 `vote_state` 中提取各个字段，并将它们转换为用户友好的 UI 格式：
//      - `epoch_credits`: 遍历 `vote_state.epoch_credits()`，将每个元组 `(epoch, credits, previous_credits)`
//        转换为 `UiEpochCredits` 结构（其中 `credits` 和 `previous_credits` 转为字符串）。
//      - `votes`: 遍历 `vote_state.votes` (这是一个 `VecDeque<Lockout>`)，将每个 `Lockout`
//        转换为 `UiLockout` 结构（提取 `slot` 和 `confirmation_count`）。
//      - `authorized_voters`: 遍历 `vote_state.authorized_voters()`，将每个元组 `(epoch, authorized_voter_pubkey)`
//        转换为 `UiAuthorizedVoters` 结构（其中 `authorized_voter_pubkey` 转为字符串）。
//      - `prior_voters`: 遍历 `vote_state.prior_voters().buf()` (这是一个环形缓冲区 `CircBuf`)，
//        过滤掉其中公钥为默认值（全零）的无效条目，然后将每个有效条目
//        `(authorized_pubkey, epoch_of_last_authorized_switch, target_epoch)`
//        转换为 `UiPriorVoters` 结构（其中 `authorized_pubkey` 转为字符串）。
//   3. 使用这些转换后的 UI 格式数据，以及从 `vote_state` 直接获取并转换为字符串的 `node_pubkey` 和 `authorized_withdrawer`，
//      还有直接复制的 `commission`、`root_slot` 和 `last_timestamp`，来构建一个 `UiVoteState` 结构。
//   4. 将这个 `UiVoteState` 包装在 `VoteAccountType::Vote` 枚举成员中，并作为 `Ok` 结果返回。
pub fn parse_vote(data: &[u8]) -> Result<VoteAccountType, ParseAccountError> {
    // 1. 尝试将原始数据反序列化为 VoteState 结构。
    //    `map_err(ParseAccountError::from)` 将底层的反序列化错误转换为 ParseAccountError。
    let mut vote_state = VoteState::deserialize(data).map_err(ParseAccountError::from)?;

    // 2. 转换 EpochCredits 列表为 UI 格式。
    //    `epoch_credits()` 返回一个迭代器，每个元素是 (Epoch, u64, u64) 的元组。
    let epoch_credits = vote_state
        .epoch_credits() // 获取 Epoch 信用点数记录
        .iter()          // 转换为迭代器
        .map(|(epoch, credits, previous_credits)| UiEpochCredits { // 对每个记录进行映射
            epoch: *epoch,                           // Epoch 直接使用
            credits: credits.to_string(),            // u64 转 String
            previous_credits: previous_credits.to_string(), // u64 转 String
        })
        .collect(); // 收集成 Vec<UiEpochCredits>

    // 3. 转换 Votes (Lockout) 列表为 UI 格式。
    //    `vote_state.votes` 是一个 VecDeque<Lockout>。
    let votes = vote_state
        .votes
        .iter() // 转换为迭代器
        .map(|lockout| UiLockout { // 对每个 Lockout进行映射
            slot: lockout.slot(),                         // Slot 直接使用
            confirmation_count: lockout.confirmation_count(), // u32 直接使用
        })
        .collect(); // 收集成 Vec<UiLockout>

    // 4. 转换 AuthorizedVoters 列表为 UI 格式。
    //    `authorized_voters()` 返回一个迭代器，每个元素是 (Epoch, Pubkey) 的元组。
    let authorized_voters = vote_state
        .authorized_voters() // 获取授权投票者记录
        .iter()              // 转换为迭代器
        .map(|(epoch, authorized_voter)| UiAuthorizedVoters { // 对每个记录进行映射
            epoch: *epoch,                               // Epoch 直接使用
            authorized_voter: authorized_voter.to_string(), // Pubkey 转 String
        })
        .collect(); // 收集成 Vec<UiAuthorizedVoters>

    // 5. 转换 PriorVoters 列表为 UI 格式。
    //    `prior_voters().buf()` 返回一个环形缓冲区的迭代器。
    //    每个元素是 `(Pubkey, Epoch, Epoch)` 元组。
    let prior_voters = vote_state
        .prior_voters()
        .buf() // 获取环形缓冲区
        .iter() // 转换为迭代器
        .filter(|(pubkey, _, _)| pubkey != &Pubkey::default()) // 过滤掉公钥为默认值的无效条目
        .map(
            |(authorized_pubkey, epoch_of_last_authorized_switch, target_epoch)| UiPriorVoters {
                authorized_pubkey: authorized_pubkey.to_string(), // Pubkey 转 String
                epoch_of_last_authorized_switch: *epoch_of_last_authorized_switch, // Epoch 直接使用
                target_epoch: *target_epoch,                     // Epoch 直接使用
            },
        )
        .collect(); // 收集成 Vec<UiPriorVoters>

    // 6. 构建 UiVoteState 并返回。
    Ok(VoteAccountType::Vote(UiVoteState {
        node_pubkey: vote_state.node_pubkey.to_string(), // 验证者节点身份公钥
        authorized_withdrawer: vote_state.authorized_withdrawer.to_string(), // 授权提款者公钥
        commission: vote_state.commission,             // 佣金百分比 (u8)
        votes,                                         // 已转换的投票列表
        root_slot: vote_state.root_slot,               // 根 Slot (Option<Slot>)
        authorized_voters,                             // 已转换的授权投票者列表
        prior_voters,                                  // 已转换的历史投票者列表
        epoch_credits,                                 // 已转换的 Epoch 信用点数列表
        last_timestamp: vote_state.last_timestamp,     // 最后投票时间戳 (BlockTimestamp)
    }))
}

// 枚举 `VoteAccountType`:
// 一个包装枚举，用于在解析结果中保持与其他账户类型（如Token, Stake等）的一致性。
// 目前它只有一个成员 `Vote`，其中包含了 `UiVoteState` 数据。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`:
//   - `Debug`: 允许调试打印。
//   - `Serialize`, `Deserialize`: 允许 `serde` 进行序列化和反序列化。
//   - `PartialEq`, `Eq`: 允许比较实例是否相等（主要用于测试）。
// `#[serde(rename_all = "camelCase", tag = "type", content = "info")]`:
//   - `rename_all = "camelCase"`: JSON 字段和枚举成员名使用驼峰式。
//   - `tag = "type"`: JSON 中会有一个 "type" 字段，值为 "vote"。
//   - `content = "info"`: `UiVoteState` 数据会放在 "info" 字段中。
/// A wrapper enum for consistency across programs
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum VoteAccountType {
    Vote(UiVoteState), // 表示这是一个投票账户，包含 `UiVoteState` 数据。
}

// 结构体 `UiVoteState`:
// 这是 `VoteState` (链上原始投票账户状态) 的一个用户界面友好的副本/表示。
// 它将原始数据（如公钥、大数字）转换为字符串等更易读的格式。
// `#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]`:
//   - `Default`: 允许创建此结构体的默认实例。
/// A duplicate representation of VoteState for pretty JSON serialization
#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiVoteState {
    node_pubkey: String,            // 验证者节点的身份公钥（字符串）。
    authorized_withdrawer: String,  // 授权提款者的公钥（字符串）。
    commission: u8,                 // 验证者收取的佣金百分比。
    votes: Vec<UiLockout>,          // 最近的投票记录列表，每个元素是 `UiLockout`。
    root_slot: Option<Slot>,        // 验证者当前认为的根 Slot。`Option<Slot>` 表示可能没有（例如新启动的验证者）。
    authorized_voters: Vec<UiAuthorizedVoters>, // 授权投票者列表。
    prior_voters: Vec<UiPriorVoters>,           // 历史授权投票者列表。
    epoch_credits: Vec<UiEpochCredits>,         // 各个 Epoch 获得的信用点数列表。
    last_timestamp: BlockTimestamp, // 最后一次投票的时间戳和 Slot。
}

// 结构体 `UiLockout`:
// 用户友好的投票锁定记录表示。对应原始的 `Lockout` 结构。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct UiLockout {
    slot: Slot,             // 投票的 Slot (槽) 号。
    confirmation_count: u32, // 该投票获得的确认数。
}

// 实现 `From<&Lockout> for UiLockout`:
// 定义如何从原始的链上 `Lockout` 结构体（的引用）转换为用户友好的 `UiLockout`。
// 注意：这里参数是 `&Lockout` (引用)，因为 `vote_state.votes` 迭代器返回的是引用。
impl From<&Lockout> for UiLockout {
    fn from(lockout: &Lockout) -> Self {
        Self {
            slot: lockout.slot(), // 直接调用原始 lockout 的方法获取 slot
            confirmation_count: lockout.confirmation_count(), // 直接调用原始 lockout 的方法获取确认数
        }
    }
}

// 结构体 `UiAuthorizedVoters`:
// 用户友好的授权投票者记录表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct UiAuthorizedVoters {
    epoch: Epoch,               // 此授权生效的 Epoch (时期)。
    authorized_voter: String,   // 在该 Epoch 有权投票的账户公钥（字符串）。
}

// 结构体 `UiPriorVoters`:
// 用户友好的历史授权投票者记录表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct UiPriorVoters {
    authorized_pubkey: String, // 历史授权投票者的公钥（字符串）。
    epoch_of_last_authorized_switch: Epoch, // 此投票者最后一次被设置为授权的 Epoch。
    target_epoch: Epoch,        // 此授权记录的目标 Epoch (通常是它失效或被取代的 Epoch)。
}

// 结构体 `UiEpochCredits`:
// 用户友好的 Epoch 信用点数记录表示。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct UiEpochCredits {
    epoch: Epoch,                   // 对应的 Epoch (时期)。
    credits: StringAmount,          // 在该 Epoch 获得的信用点数（字符串表示）。
    previous_credits: StringAmount, // 在上一个 Epoch 结束时该验证者拥有的信用点数（字符串表示）。
}

// 测试模块
#[cfg(test)]
mod test {
    // 导入父模块的所有项 (`super::*`) 和测试中需要的特定项。
    use {super::*, solana_vote_interface::state::VoteStateVersions};
    // `VoteStateVersions` 用于包装 `VoteState` 以处理版本兼容性，在序列化到账户数据时使用。

    // 测试函数 `test_parse_vote`
    // `#[test]` 宏标记这是一个单元测试函数。
    #[test]
    fn test_parse_vote() {
        // 1. 测试默认 VoteState 的解析
        let vote_state_default = VoteState::default(); // 创建一个默认的 VoteState 实例。
        // 创建一个足够大的字节向量来存储序列化后的 VoteState。
        let mut vote_account_data_bytes = vec![0; VoteState::size_of()];
        // 将 VoteState 包装在版本控制结构中。
        let versioned_vote_state = VoteStateVersions::new_current(vote_state_default);
        // 将版本化的 VoteState 序列化到字节向量中。
        VoteState::serialize(&versioned_vote_state, &mut vote_account_data_bytes).unwrap();

        // 定义预期的解析结果 (UiVoteState 格式)。
        // 对于默认的 VoteState，node_pubkey 和 authorized_withdrawer 都是 Pubkey::default()。
        // 其他字段（如 votes, epoch_credits 等列表）在默认情况下为空。
        let expected_ui_vote_state = UiVoteState {
            node_pubkey: Pubkey::default().to_string(),
            authorized_withdrawer: Pubkey::default().to_string(),
            ..UiVoteState::default() // 其余字段使用 UiVoteState 的默认值 (例如空列表)。
        };

        // 调用 `parse_vote` 函数解析序列化后的数据。
        // `.unwrap()` 用于获取 `Result` 中的 `Ok` 值，如果解析失败则测试会 panic。
        let parsed_result = parse_vote(&vote_account_data_bytes).unwrap();

        // 断言：解析结果应该等于预期的 `VoteAccountType::Vote`，其内部包含转换后的 `expected_ui_vote_state`。
        assert_eq!(
            parsed_result,
            VoteAccountType::Vote(expected_ui_vote_state)
        );

        // 2. 测试无效数据的情况
        let bad_data = vec![0; 4]; // 创建一些明显不是有效投票账户数据的字节 (长度也不符)。
        // 尝试用这些坏数据解析，并断言结果是 `Err`。
        // `.is_err()` 检查 `Result` 是否为 `Err` 变体。
        assert!(parse_vote(&bad_data).is_err());
    }
}
