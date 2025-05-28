// 文件功能说明：
// 这个文件是 Solana `account-decoder` 库中一个关键的辅助模块，专门负责解析
// SPL Token 2022 (也常简称为 Token-22) 标准中引入的各种“扩展”(Extensions)。
// SPL Token 2022 是对原始 SPL Token 标准的重大升级，它通过扩展机制，
// 允许代币的 Mint (铸币/定义) 账户或 Token (持有者) 账户拥有更多定制化的功能和行为。
//
// 本文件的核心功能由 `parse_extension` 函数提供。这个函数接收一个扩展类型 (`ExtensionType`)
// 和一个包含扩展数据的账户状态 (`StateWithExtensions<S>`) 作为输入。
// 然后，它会根据具体的 `ExtensionType`，从账户状态中提取出对应的原始扩展数据结构，
// 并调用该扩展专属的转换函数 (例如 `convert_transfer_fee_config`, `convert_interest_bearing_config` 等)，
// 将这些链上存储的原始数据转换为用户友好 (UI-friendly) 的结构体。
//
// 这些 UI 结构体 (例如 `UiTransferFeeConfig`, `UiInterestBearingConfig`) 通常会将原始数据
// (如公钥 `Pubkey`、大整数 `u64`/`i16`、布尔值、甚至是复杂的加密数据结构如 `PodElGamalPubkey`)
// 转换为字符串 (`String`) 或其他更易于人类阅读和 JSON 序列化的格式。
//
// 支持解析的扩展种类繁多，涵盖了例如：
// - 转账费用 (Transfer Fee): 允许代币在转账时收取一定比例的费用。
// - 铸币关闭权限 (Mint Close Authority): 允许指定一个地址有权关闭 Mint 账户。
// - 机密转账 (Confidential Transfers): 使用零知识证明技术实现代币持有量和转账金额的隐私保护。
// - 默认账户状态 (Default Account State): 允许 Mint 定义其新创建的 Token Account 的默认状态 (例如默认冻结)。
// - 不可变所有者 (Immutable Owner): Token Account 一旦被初始化，其所有者就不能再更改。
// - 转账备注要求 (Memo Transfer): 要求某些转账必须附带备注信息。
// - 生息代币 (Interest-Bearing Tokens): 允许代币的余额随时间自动增长。
// - CPI守卫 (CPI Guard): 限制哪些程序可以通过跨程序调用 (Cross-Program Invocation) 来与此代币账户交互。
// - 永久委托 (Permanent Delegate): 允许设置一个永久的委托权限，该权限不能被撤销。
// - 元数据指针 (Metadata Pointer): 指向存储代币元数据 (如名称、符号、URI) 的账户地址。
// - 代币元数据 (Token Metadata): 直接在 Mint 账户中存储代币的元数据。
// - 转账钩子 (Transfer Hook): 允许在每次代币转账时调用一个指定的程序，实现自定义逻辑 (如版税、黑名单等)。
// - 代币组 (Token Group): 用于将一组 NFT (非同质化代币) 组织成一个逻辑上的集合。
// - 可暂停 (Pausable): 允许代币的某些活动 (如转账) 被一个授权地址暂停。
//
// 总之，`parse_token_extension.rs` 使得复杂的 Token-2022 扩展功能能够被解码和呈现给用户和开发者，
// 是理解和使用这些高级代币特性的重要工具。

// 重新导出在 `solana-account-decoder-client-types` crate 中定义的、所有与 Token 扩展相关的 UI 结构体。
// 这样做使得使用 `account-decoder` 的其他代码可以直接通过 `crate::parse_token_extension::UiTransferFeeConfig` 等方式访问这些类型，
// 而无需显式依赖 `solana-account-decoder-client-types`。
pub use solana_account_decoder_client_types::token::{
    UiConfidentialMintBurn, UiConfidentialTransferAccount, UiConfidentialTransferFeeAmount,
    UiConfidentialTransferFeeConfig, UiConfidentialTransferMint, UiCpiGuard, UiDefaultAccountState,
    UiExtension, UiGroupMemberPointer, UiGroupPointer, UiInterestBearingConfig, UiMemoTransfer,
    UiMetadataPointer, UiMintCloseAuthority, UiPausableConfig, UiPermanentDelegate,
    UiScaledUiAmountConfig, UiTokenGroup, UiTokenGroupMember, UiTokenMetadata, UiTransferFee,
    UiTransferFeeAmount, UiTransferFeeConfig, UiTransferHook, UiTransferHookAccount,
};
use {
    crate::parse_token::convert_account_state, // 从当前 crate 的 `parse_token` 模块导入 `convert_account_state` 函数，用于转换账户状态。
    solana_clock::UnixTimestamp,                // 从 Solana 时钟库导入 `UnixTimestamp` 类型。
    solana_program_pack::Pack,                  // 导入 `Pack` 特性，用于在 Rust 结构体和字节数组之间进行序列化/反序列化。
    solana_pubkey::Pubkey,                      // 导入 `Pubkey` 类型，表示 Solana 账户地址。
    spl_token_2022::{                           // 从 SPL Token 2022 标准库导入。
        extension::{self, BaseState, BaseStateWithExtensions, ExtensionType, StateWithExtensions}, // `extension` 模块本身，以及处理扩展的核心类型：
                                                                                                    // `BaseState`: 表示一个基础状态结构 (如 Mint 或 Account) 必须满足的特性。
                                                                                                    // `BaseStateWithExtensions`: 用于访问带有扩展的基础状态的特性。
                                                                                                    // `ExtensionType`: 枚举，列出了所有已知的扩展类型。
                                                                                                    // `StateWithExtensions`: 包装了基础状态和其扩展数据的结构。
        solana_zk_sdk::encryption::pod::elgamal::PodElGamalPubkey, // 从 Solana 零知识证明 SDK 导入 ElGamal 公钥的 POD (Plain Old Data) 版本，用于机密转账。
    },
    spl_token_group_interface::state::{TokenGroup, TokenGroupMember}, // 从 SPL Token Group 接口库导入 TokenGroup 和 TokenGroupMember 的链上状态结构。
    spl_token_metadata_interface::state::TokenMetadata, // 从 SPL Token Metadata 接口库导入 TokenMetadata 的链上状态结构。
};

// 函数 `parse_extension`：
// 功能：根据给定的扩展类型，从包含扩展的账户状态中提取并解析该扩展的数据，将其转换为用户友好的 UI 结构。
// 泛型参数 `S`:
//   - `S` 代表基础状态类型，例如 `spl_token_2022::state::Mint` 或 `spl_token_2022::state::Account`。
//   - 约束 `S: BaseState + Pack` 表示类型 `S` 必须实现了 `BaseState` 特性 (表明它是一个可以附加扩展的基础状态)
//     并且实现了 `Pack` 特性 (表明它可以被序列化/反序列化)。
// 参数：
//   - `extension_type`: `&ExtensionType`，一个引用，指向要解析的扩展的类型。
//   - `account`: `&StateWithExtensions<S>`，一个引用，指向包含基础状态 `S` 及其所有扩展数据的账户状态对象。
// 返回值：`UiExtension`，一个枚举，包含了对应于 `extension_type` 的、已转换为 UI 格式的扩展数据。
//         如果无法解析或扩展不存在，则返回 `UiExtension::UnparseableExtension` 或 `UiExtension::Uninitialized`。
// 设计思路：
//   1. 使用 `match` 表达式对传入的 `extension_type` 进行分支处理。
//   2. 对于每一种已知的 `ExtensionType`：
//      a. 调用 `account.get_extension::<OriginalExtensionStruct>()` 方法尝试从 `account` 中获取该类型的原始扩展数据。
//         例如，对于 `ExtensionType::TransferFeeConfig`，会尝试获取 `extension::transfer_fee::TransferFeeConfig` 类型的数据。
//         `get_extension` 返回一个 `Result`，如果扩展存在且类型匹配，则为 `Ok(&OriginalExtensionStruct)`。
//      b. 使用 `.map(|&extension| ...)` 处理成功获取到的原始扩展数据 `extension`：
//         i.  调用该扩展专属的转换函数 (例如 `convert_transfer_fee_config(extension)`)，将原始结构转换为对应的 UI 结构。
//         ii. 将转换后的 UI 结构包装在 `UiExtension` 枚举的相应成员中 (例如 `UiExtension::TransferFeeConfig(...)`)。
//      c. 如果 `get_extension` 失败（例如扩展不存在或类型不匹配，虽然 `StateWithExtensions` 内部通常会保证类型安全），
//         或者转换函数内部发生错误（比较少见，因为转换通常是直接的数据映射），
//         则 `.unwrap_or(UiExtension::UnparseableExtension)` 会捕获这个 `None` (由 `.ok()` 转换而来) 或错误，
//         并返回一个表示无法解析的 `UiExtension::UnparseableExtension`。
//   3. 对于某些简单的、没有额外数据的标志性扩展 (例如 `ImmutableOwner`, `NonTransferable`)，可以直接映射到 `UiExtension` 中的对应成员。
//   4. 特别地，对于可变长度的扩展 (如 `TokenMetadata`)，使用 `account.get_variable_len_extension::<TokenMetadata>()` 来获取。
//   5. 如果 `extension_type` 是 `ExtensionType::Uninitialized`，则直接返回 `UiExtension::Uninitialized`。
pub fn parse_extension<S: BaseState + Pack>(
    extension_type: &ExtensionType, // 要解析的扩展类型
    account: &StateWithExtensions<S>, // 包含基础状态和所有扩展的账户对象
) -> UiExtension {
    match extension_type {
        ExtensionType::Uninitialized => UiExtension::Uninitialized, // 未初始化的扩展类型
        // 转账费用配置扩展
        ExtensionType::TransferFeeConfig => account
            .get_extension::<extension::transfer_fee::TransferFeeConfig>() // 尝试获取原始 TransferFeeConfig 扩展数据
            .map(|&extension| { // 如果成功获取 (`Some(&extension)`)
                // 调用转换函数，并包装到 UiExtension::TransferFeeConfig 中
                UiExtension::TransferFeeConfig(convert_transfer_fee_config(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension), // 如果获取失败或 map 内部返回 None，则视为无法解析
        // 转账费用金额扩展 (通常在 Token Account 中，表示已累积的未提取费用)
        ExtensionType::TransferFeeAmount => account
            .get_extension::<extension::transfer_fee::TransferFeeAmount>()
            .map(|&extension| {
                UiExtension::TransferFeeAmount(convert_transfer_fee_amount(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // Mint 关闭权限扩展
        ExtensionType::MintCloseAuthority => account
            .get_extension::<extension::mint_close_authority::MintCloseAuthority>()
            .map(|&extension| {
                UiExtension::MintCloseAuthority(convert_mint_close_authority(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 机密转账 - Mint 配置扩展
        ExtensionType::ConfidentialTransferMint => account
            .get_extension::<extension::confidential_transfer::ConfidentialTransferMint>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferMint(convert_confidential_transfer_mint(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 机密转账 - 费用配置扩展
        ExtensionType::ConfidentialTransferFeeConfig => account
            .get_extension::<extension::confidential_transfer_fee::ConfidentialTransferFeeConfig>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferFeeConfig(
                    convert_confidential_transfer_fee_config(extension),
                )
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 机密转账 - Account 配置扩展
        ExtensionType::ConfidentialTransferAccount => account
            .get_extension::<extension::confidential_transfer::ConfidentialTransferAccount>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferAccount(convert_confidential_transfer_account(
                    extension,
                ))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 机密转账 - 费用金额扩展
        ExtensionType::ConfidentialTransferFeeAmount => account
            .get_extension::<extension::confidential_transfer_fee::ConfidentialTransferFeeAmount>()
            .map(|&extension| {
                UiExtension::ConfidentialTransferFeeAmount(
                    convert_confidential_transfer_fee_amount(extension),
                )
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 默认账户状态扩展 (在 Mint 上配置，影响新创建的 Token Account)
        ExtensionType::DefaultAccountState => account
            .get_extension::<extension::default_account_state::DefaultAccountState>()
            .map(|&extension| {
                UiExtension::DefaultAccountState(convert_default_account_state(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 不可变所有者扩展 (在 Token Account 上，一旦设置，owner 不能更改)
        ExtensionType::ImmutableOwner => UiExtension::ImmutableOwner, // 这是一个标志性扩展，没有额外数据
        // 转账备注要求扩展
        ExtensionType::MemoTransfer => account
            .get_extension::<extension::memo_transfer::MemoTransfer>()
            .map(|&extension| UiExtension::MemoTransfer(convert_memo_transfer(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 不可转让代币扩展 (在 Mint 上配置)
        ExtensionType::NonTransferable => UiExtension::NonTransferable, // 标志性扩展
        // 生息代币配置扩展 (在 Mint 上配置)
        ExtensionType::InterestBearingConfig => account
            .get_extension::<extension::interest_bearing_mint::InterestBearingConfig>()
            .map(|&extension| {
                UiExtension::InterestBearingConfig(convert_interest_bearing_config(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // CPI (跨程序调用) 守卫扩展 (在 Token Account 上配置)
        ExtensionType::CpiGuard => account
            .get_extension::<extension::cpi_guard::CpiGuard>()
            .map(|&extension| UiExtension::CpiGuard(convert_cpi_guard(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 永久委托扩展 (在 Token Account 上配置)
        ExtensionType::PermanentDelegate => account
            .get_extension::<extension::permanent_delegate::PermanentDelegate>()
            .map(|&extension| UiExtension::PermanentDelegate(convert_permanent_delegate(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 不可转让的 Token Account 扩展 (与 Mint 上的 NonTransferable 配合使用)
        ExtensionType::NonTransferableAccount => UiExtension::NonTransferableAccount, // 标志性扩展
        // 元数据指针扩展
        ExtensionType::MetadataPointer => account
            .get_extension::<extension::metadata_pointer::MetadataPointer>()
            .map(|&extension| UiExtension::MetadataPointer(convert_metadata_pointer(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 代币元数据扩展 (直接在 Mint 中存储元数据，是可变长度的扩展)
        ExtensionType::TokenMetadata => account
            .get_variable_len_extension::<TokenMetadata>() // 使用 get_variable_len_extension
            .map(|extension| UiExtension::TokenMetadata(convert_token_metadata(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 转账钩子扩展 (在 Mint 上配置，指定一个程序在每次转账时被调用)
        ExtensionType::TransferHook => account
            .get_extension::<extension::transfer_hook::TransferHook>()
            .map(|&extension| UiExtension::TransferHook(convert_transfer_hook(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 转账钩子账户扩展 (在 Token Account 上，与 Mint 上的 TransferHook 配合)
        ExtensionType::TransferHookAccount => account
            .get_extension::<extension::transfer_hook::TransferHookAccount>()
            .map(|&extension| {
                UiExtension::TransferHookAccount(convert_transfer_hook_account(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 代币组指针扩展 (在 Mint 上，指向一个 TokenGroup Mint)
        ExtensionType::GroupPointer => account
            .get_extension::<extension::group_pointer::GroupPointer>()
            .map(|&extension| UiExtension::GroupPointer(convert_group_pointer(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 代币组成员指针扩展 (在 Mint 上，指向一个 TokenGroupMember Mint)
        ExtensionType::GroupMemberPointer => account
            .get_extension::<extension::group_member_pointer::GroupMemberPointer>()
            .map(|&extension| {
                UiExtension::GroupMemberPointer(convert_group_member_pointer(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 代币组扩展 (在 Mint 上，定义一个 NFT 组)
        ExtensionType::TokenGroup => account
            .get_extension::<TokenGroup>() // TokenGroup 是外部 crate spl_token_group_interface 定义的
            .map(|&extension| UiExtension::TokenGroup(convert_token_group(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 代币组成员扩展 (在 Mint 上，将一个 NFT 标记为某个组的成员)
        ExtensionType::TokenGroupMember => account
            .get_extension::<TokenGroupMember>() // TokenGroupMember 也是外部 crate 定义的
            .map(|&extension| UiExtension::TokenGroupMember(convert_token_group_member(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 机密铸币/销毁扩展 (在 ConfidentialTransferMint 扩展上进一步添加)
        ExtensionType::ConfidentialMintBurn => account
            .get_extension::<extension::confidential_mint_burn::ConfidentialMintBurn>()
            .map(|&extension| {
                UiExtension::ConfidentialMintBurn(convert_confidential_mint_burn(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // UI 金额缩放配置扩展 (在 Mint 上配置，用于调整代币在 UI 上的显示方式)
        ExtensionType::ScaledUiAmount => account
            .get_extension::<extension::scaled_ui_amount::ScaledUiAmountConfig>()
            .map(|&extension| {
                UiExtension::ScaledUiAmountConfig(convert_scaled_ui_amount(extension))
            })
            .unwrap_or(UiExtension::UnparseableExtension),
        // 可暂停扩展 (在 Mint 上配置，允许暂停代币的某些操作)
        ExtensionType::Pausable => account
            .get_extension::<extension::pausable::PausableConfig>()
            .map(|&extension| UiExtension::PausableConfig(convert_pausable_config(extension)))
            .unwrap_or(UiExtension::UnparseableExtension),
        // 可暂停账户扩展 (在 Token Account 上，与 Mint 上的 Pausable 配合)
        ExtensionType::PausableAccount => UiExtension::PausableAccount, // 标志性扩展
    }
}

// --- 下面是每种具体扩展的转换函数 ---
// 这些函数都遵循类似的模式：接收原始扩展结构体的引用，提取其字段，
// 将需要转换的字段（如 Pubkey, COption<Pubkey>, 大整数, 枚举等）转换为字符串或对应的 UI 类型，
// 然后填充到相应的 UiExtension 子结构体中。

// 辅助函数，转换 `extension::transfer_fee::TransferFee` 到 `UiTransferFee`
fn convert_transfer_fee(transfer_fee: extension::transfer_fee::TransferFee) -> UiTransferFee {
    UiTransferFee {
        epoch: u64::from(transfer_fee.epoch), // Epoch (u64) 直接用
        maximum_fee: u64::from(transfer_fee.maximum_fee), // u64 金额直接用 (但 UiTransferFee 可能将其显示为字符串)
        transfer_fee_basis_points: u16::from(transfer_fee.transfer_fee_basis_points), // u16 基点值直接用
    }
}

// 转换 TransferFeeConfig 扩展
fn convert_transfer_fee_config(
    transfer_fee_config: extension::transfer_fee::TransferFeeConfig,
) -> UiTransferFeeConfig {
    // `into()`: COption<Pubkey> -> Option<Pubkey>
    let transfer_fee_config_authority: Option<Pubkey> =
        transfer_fee_config.transfer_fee_config_authority.into();
    let withdraw_withheld_authority: Option<Pubkey> =
        transfer_fee_config.withdraw_withheld_authority.into();

    UiTransferFeeConfig {
        // `map(|pubkey| pubkey.to_string())`: Option<Pubkey> -> Option<String>
        transfer_fee_config_authority: transfer_fee_config_authority
            .map(|pubkey| pubkey.to_string()),
        withdraw_withheld_authority: withdraw_withheld_authority.map(|pubkey| pubkey.to_string()),
        withheld_amount: u64::from(transfer_fee_config.withheld_amount), // 从 PodU64 转换为 u64
        older_transfer_fee: convert_transfer_fee(transfer_fee_config.older_transfer_fee), // 递归转换内部的 TransferFee
        newer_transfer_fee: convert_transfer_fee(transfer_fee_config.newer_transfer_fee),
    }
}

// 转换 TransferFeeAmount 扩展
fn convert_transfer_fee_amount(
    transfer_fee_amount: extension::transfer_fee::TransferFeeAmount,
) -> UiTransferFeeAmount {
    UiTransferFeeAmount {
        withheld_amount: u64::from(transfer_fee_amount.withheld_amount), // PodU64 -> u64
    }
}

// 转换 MintCloseAuthority 扩展
fn convert_mint_close_authority(
    mint_close_authority: extension::mint_close_authority::MintCloseAuthority,
) -> UiMintCloseAuthority {
    let authority: Option<Pubkey> = mint_close_authority.close_authority.into(); // COption<Pubkey> -> Option<Pubkey>
    UiMintCloseAuthority {
        close_authority: authority.map(|pubkey| pubkey.to_string()), // Option<Pubkey> -> Option<String>
    }
}

// 转换 DefaultAccountState 扩展
fn convert_default_account_state(
    default_account_state: extension::default_account_state::DefaultAccountState,
) -> UiDefaultAccountState {
    // 原始的 `default_account_state.state` 是 `u8` 类型。
    // 需要尝试将其转换为 `spl_token_2022::state::AccountState` 枚举。
    let account_state_enum = spl_token_2022::state::AccountState::try_from(default_account_state.state)
        .unwrap_or_default(); // 如果转换失败（例如u8值无效），则默认为 AccountState::Uninitialized (或 AccountState 的 Default 实现)。
    UiDefaultAccountState {
        // 然后将 `AccountState` 枚举转换为 `UiAccountState` 枚举。
        account_state: convert_account_state(account_state_enum),
    }
}

// 转换 MemoTransfer 扩展
fn convert_memo_transfer(memo_transfer: extension::memo_transfer::MemoTransfer) -> UiMemoTransfer {
    UiMemoTransfer {
        // `memo_transfer.require_incoming_transfer_memos` 是 `PodBool` 类型。
        // `.into()` 会将其转换为标准的 `bool` 类型。
        require_incoming_transfer_memos: memo_transfer.require_incoming_transfer_memos.into(),
    }
}

// 转换 InterestBearingConfig 扩展 (生息代币配置)
fn convert_interest_bearing_config(
    interest_bearing_config: extension::interest_bearing_mint::InterestBearingConfig,
) -> UiInterestBearingConfig {
    let rate_authority: Option<Pubkey> = interest_bearing_config.rate_authority.into(); // COption<Pubkey> -> Option<Pubkey>

    UiInterestBearingConfig {
        rate_authority: rate_authority.map(|pubkey| pubkey.to_string()), // Option<Pubkey> -> Option<String>
        // PodUnixTimestamp (i64) -> UnixTimestamp (i64)
        initialization_timestamp: UnixTimestamp::from(
            interest_bearing_config.initialization_timestamp,
        ),
        // PodI16 -> i16 (利率，单位是基点，例如 500 表示 5%)
        pre_update_average_rate: i16::from(interest_bearing_config.pre_update_average_rate),
        last_update_timestamp: UnixTimestamp::from(interest_bearing_config.last_update_timestamp),
        current_rate: i16::from(interest_bearing_config.current_rate),
    }
}

// 转换 CpiGuard 扩展 (CPI调用守卫)
fn convert_cpi_guard(cpi_guard: extension::cpi_guard::CpiGuard) -> UiCpiGuard {
    UiCpiGuard {
        lock_cpi: cpi_guard.lock_cpi.into(), // PodBool -> bool
    }
}

// 转换 PermanentDelegate 扩展 (永久委托)
fn convert_permanent_delegate(
    permanent_delegate: extension::permanent_delegate::PermanentDelegate,
) -> UiPermanentDelegate {
    let delegate: Option<Pubkey> = permanent_delegate.delegate.into(); // COption<Pubkey> -> Option<Pubkey>
    UiPermanentDelegate {
        delegate: delegate.map(|pubkey| pubkey.to_string()), // Option<Pubkey> -> Option<String>
    }
}

// 转换 ConfidentialTransferMint 扩展 (机密转账 - Mint 设置)
// 注意：这个函数是 `pub` 公开的，可能是因为它在 crate 外部（例如测试或其他模块）也被直接使用。
pub fn convert_confidential_transfer_mint(
    confidential_transfer_mint: extension::confidential_transfer::ConfidentialTransferMint,
) -> UiConfidentialTransferMint {
    let authority: Option<Pubkey> = confidential_transfer_mint.authority.into();
    // `auditor_elgamal_pubkey` 是用于审计的 ElGamal 公钥，用于让审计员能够解密交易信息。
    let auditor_elgamal_pubkey: Option<PodElGamalPubkey> =
        confidential_transfer_mint.auditor_elgamal_pubkey.into();
    UiConfidentialTransferMint {
        authority: authority.map(|pubkey| pubkey.to_string()),
        auto_approve_new_accounts: confidential_transfer_mint.auto_approve_new_accounts.into(), // PodBool -> bool (是否自动批准新的机密账户)
        auditor_elgamal_pubkey: auditor_elgamal_pubkey.map(|pubkey| pubkey.to_string()), // ElGamal 公钥转字符串
    }
}

// 转换 ConfidentialTransferFeeConfig 扩展 (机密转账 - 费用配置)
// 注意：这个函数也是 `pub` 公开的。
pub fn convert_confidential_transfer_fee_config(
    confidential_transfer_fee_config: extension::confidential_transfer_fee::ConfidentialTransferFeeConfig,
) -> UiConfidentialTransferFeeConfig {
    let authority: Option<Pubkey> = confidential_transfer_fee_config.authority.into();
    // `withdraw_withheld_authority_elgamal_pubkey` 是用于提取被扣留的机密转账费用的 ElGamal 公钥。
    let withdraw_withheld_authority_elgamal_pubkey: Option<PodElGamalPubkey> =
        confidential_transfer_fee_config
            .withdraw_withheld_authority_elgamal_pubkey
            .into();
    UiConfidentialTransferFeeConfig {
        authority: authority.map(|pubkey| pubkey.to_string()),
        withdraw_withheld_authority_elgamal_pubkey: withdraw_withheld_authority_elgamal_pubkey
            .map(|pubkey| pubkey.to_string()), // ElGamal 公钥转字符串
        harvest_to_mint_enabled: confidential_transfer_fee_config
            .harvest_to_mint_enabled // PodBool -> bool (是否允许将扣留的费用直接铸造到 Mint)
            .into(),
        // `withheld_amount` 是一个加密的余额 (PodAeCiphertext)，这里直接转为字符串表示其原始（可能加密的）形式。
        withheld_amount: format!("{}", confidential_transfer_fee_config.withheld_amount),
    }
}

// 转换 ConfidentialTransferAccount 扩展 (机密转账 - Token Account 设置)
fn convert_confidential_transfer_account(
    confidential_transfer_account: extension::confidential_transfer::ConfidentialTransferAccount,
) -> UiConfidentialTransferAccount {
    UiConfidentialTransferAccount {
        approved: confidential_transfer_account.approved.into(), // PodBool -> bool (此账户是否被批准进行机密转账)
        elgamal_pubkey: format!("{}", confidential_transfer_account.elgamal_pubkey), // ElGamal 公钥转字符串
        // 以下都是加密的余额或计数器，转换为字符串表示。
        pending_balance_lo: format!("{}", confidential_transfer_account.pending_balance_lo), // 待处理余额（低位部分）
        pending_balance_hi: format!("{}", confidential_transfer_account.pending_balance_hi), // 待处理余额（高位部分）
        available_balance: format!("{}", confidential_transfer_account.available_balance),   // 可用余额
        decryptable_available_balance: format!( // 可解密的可用余额 (通常由审计员或用户自己解密)
            "{}",
            confidential_transfer_account.decryptable_available_balance
        ),
        allow_confidential_credits: confidential_transfer_account // PodBool -> bool (是否允许接收机密入账)
            .allow_confidential_credits
            .into(),
        allow_non_confidential_credits: confidential_transfer_account // PodBool -> bool (是否允许接收非机密入账)
            .allow_non_confidential_credits
            .into(),
        pending_balance_credit_counter: confidential_transfer_account // PodU64 -> u64 (待处理余额的入账计数器)
            .pending_balance_credit_counter
            .into(),
        maximum_pending_balance_credit_counter: confidential_transfer_account // PodU64 -> u64
            .maximum_pending_balance_credit_counter
            .into(),
        expected_pending_balance_credit_counter: confidential_transfer_account // PodU64 -> u64
            .expected_pending_balance_credit_counter
            .into(),
        actual_pending_balance_credit_counter: confidential_transfer_account // PodU64 -> u64
            .actual_pending_balance_credit_counter
            .into(),
    }
}

// 转换 ConfidentialTransferFeeAmount 扩展 (机密转账 - 费用金额)
fn convert_confidential_transfer_fee_amount(
    confidential_transfer_fee_amount: extension::confidential_transfer_fee::ConfidentialTransferFeeAmount,
) -> UiConfidentialTransferFeeAmount {
    UiConfidentialTransferFeeAmount {
        // 加密的被扣留费用金额，转为字符串。
        withheld_amount: format!("{}", confidential_transfer_fee_amount.withheld_amount),
    }
}

// 转换 MetadataPointer 扩展 (元数据指针)
fn convert_metadata_pointer(
    metadata_pointer: extension::metadata_pointer::MetadataPointer,
) -> UiMetadataPointer {
    let authority: Option<Pubkey> = metadata_pointer.authority.into(); // 更新指针的权限
    let metadata_address: Option<Pubkey> = metadata_pointer.metadata_address.into(); // 指向元数据账户的地址
    UiMetadataPointer {
        authority: authority.map(|pubkey| pubkey.to_string()),
        metadata_address: metadata_address.map(|pubkey| pubkey.to_string()),
    }
}

// 转换 TokenMetadata 扩展 (直接存储的代币元数据)
fn convert_token_metadata(token_metadata: TokenMetadata) -> UiTokenMetadata {
    let update_authority: Option<Pubkey> = token_metadata.update_authority.into(); // 更新元数据的权限
    UiTokenMetadata {
        update_authority: update_authority.map(|pubkey| pubkey.to_string()),
        mint: token_metadata.mint.to_string(), // 此元数据对应的 Mint 地址
        name: token_metadata.name,             // 代币名称
        symbol: token_metadata.symbol,         // 代币符号
        uri: token_metadata.uri,               // 指向更详细元数据 (如图片) 的 URI
        additional_metadata: token_metadata.additional_metadata, // 其他自定义的元数据键值对
    }
}

// 转换 TransferHook 扩展 (转账钩子 - Mint 配置)
fn convert_transfer_hook(transfer_hook: extension::transfer_hook::TransferHook) -> UiTransferHook {
    let authority: Option<Pubkey> = transfer_hook.authority.into(); // 更新钩子配置的权限
    let program_id: Option<Pubkey> = transfer_hook.program_id.into(); // 转账时要调用的程序ID
    UiTransferHook {
        authority: authority.map(|pubkey| pubkey.to_string()),
        program_id: program_id.map(|pubkey| pubkey.to_string()),
    }
}

// 转换 TransferHookAccount 扩展 (转账钩子 - Token Account 状态)
fn convert_transfer_hook_account(
    transfer_hook: extension::transfer_hook::TransferHookAccount,
) -> UiTransferHookAccount {
    UiTransferHookAccount {
        transferring: transfer_hook.transferring.into(), // PodBool -> bool (此账户当前是否正在进行一个被钩子处理的转账)
    }
}

// 转换 GroupPointer 扩展 (代币组指针)
fn convert_group_pointer(group_pointer: extension::group_pointer::GroupPointer) -> UiGroupPointer {
    let authority: Option<Pubkey> = group_pointer.authority.into(); // 更新指针的权限
    let group_address: Option<Pubkey> = group_pointer.group_address.into(); // 指向 TokenGroup Mint 的地址
    UiGroupPointer {
        authority: authority.map(|pubkey| pubkey.to_string()),
        group_address: group_address.map(|pubkey| pubkey.to_string()),
    }
}

// 转换 GroupMemberPointer 扩展 (代币组成员指针)
fn convert_group_member_pointer(
    member_pointer: extension::group_member_pointer::GroupMemberPointer,
) -> UiGroupMemberPointer {
    let authority: Option<Pubkey> = member_pointer.authority.into(); // 更新指针的权限
    let member_address: Option<Pubkey> = member_pointer.member_address.into(); // 指向 TokenGroupMember Mint 的地址
    UiGroupMemberPointer {
        authority: authority.map(|pubkey| pubkey.to_string()),
        member_address: member_address.map(|pubkey| pubkey.to_string()),
    }
}

// 转换 TokenGroup 扩展 (定义一个代币组)
fn convert_token_group(token_group: TokenGroup) -> UiTokenGroup {
    let update_authority: Option<Pubkey> = token_group.update_authority.into(); // 更新组的权限
    UiTokenGroup {
        update_authority: update_authority.map(|pubkey| pubkey.to_string()),
        mint: token_group.mint.to_string(), // 此 TokenGroup 对应的 Mint 地址
        size: token_group.size.into(),       // PodU32 -> u32 (当前组内成员数量)
        max_size: token_group.max_size.into(), // PodU32 -> u32 (组内最大成员数量)
    }
}

// 转换 TokenGroupMember 扩展 (将一个 Mint 标记为某个组的成员)
fn convert_token_group_member(member: TokenGroupMember) -> UiTokenGroupMember {
    UiTokenGroupMember {
        mint: member.mint.to_string(),   // 此成员 Mint 的地址
        group: member.group.to_string(), // 所属 TokenGroup Mint 的地址
        member_number: member.member_number.into(), // PodU32 -> u32 (成员在此组内的编号)
    }
}

// 转换 ConfidentialMintBurn 扩展 (机密铸币/销毁)
fn convert_confidential_mint_burn(
    confidential_mint_burn: extension::confidential_mint_burn::ConfidentialMintBurn,
) -> UiConfidentialMintBurn {
    UiConfidentialMintBurn {
        // 以下都是加密的数值或 ElGamal 公钥，转换为字符串表示。
        confidential_supply: confidential_mint_burn.confidential_supply.to_string(), // 加密的供应量
        decryptable_supply: confidential_mint_burn.decryptable_supply.to_string(),   // 可解密的供应量 (对审计员)
        supply_elgamal_pubkey: confidential_mint_burn.supply_elgamal_pubkey.to_string(), // 供应量对应的 ElGamal 公钥
        pending_burn: confidential_mint_burn.pending_burn.to_string(),               // 待销毁的加密金额
    }
}

// 转换 ScaledUiAmountConfig 扩展 (UI 金额缩放配置)
fn convert_scaled_ui_amount(
    scaled_ui_amount_config: extension::scaled_ui_amount::ScaledUiAmountConfig,
) -> UiScaledUiAmountConfig {
    let authority: Option<Pubkey> = scaled_ui_amount_config.authority.into(); // 更新配置的权限
    let multiplier: f64 = scaled_ui_amount_config.multiplier.into(); // 当前的缩放因子 (PodF64 -> f64)
    let new_multiplier_effective_timestamp: i64 = scaled_ui_amount_config // 新缩放因子生效的Unix时间戳 (PodI64 -> i64)
        .new_multiplier_effective_timestamp
        .into();
    let new_multiplier: f64 = scaled_ui_amount_config.new_multiplier.into(); // 将要生效的新缩放因子 (PodF64 -> f64)
    UiScaledUiAmountConfig {
        authority: authority.map(|pubkey| pubkey.to_string()),
        multiplier: multiplier.to_string(), // f64 转字符串
        new_multiplier_effective_timestamp,
        new_multiplier: new_multiplier.to_string(), // f64 转字符串
    }
}

// 转换 PausableConfig 扩展 (可暂停配置 - 在 Mint 上)
fn convert_pausable_config(
    pausable_config: extension::pausable::PausableConfig,
) -> UiPausableConfig {
    let authority: Option<Pubkey> = pausable_config.authority.into(); // 暂停/恢复操作的权限
    UiPausableConfig {
        authority: authority.map(|pubkey| pubkey.to_string()),
        paused: pausable_config.paused.into(), // PodBool -> bool (当前是否已暂停)
    }
}
