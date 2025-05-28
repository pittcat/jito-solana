// 文件功能说明：
// 这个文件是 Solana `account-decoder` 库中专门用于解析 SPL Token (Solana Program Library Token)
// 及 SPL Token 2022 标准相关账户数据的核心模块。SPL Token 是 Solana 区块链上发行和管理
// 同质化代币 (fungible tokens, 类似 ERC-20) 和非同质化代币 (NFTs, 类似 ERC-721/ERC-1155) 的标准程序。
//
// 本文件主要能解析以下三种类型的 SPL Token 相关账户：
// 1. Mint Account (铸币账户/代币定义账户):
//    - 定义了一种特定代币的所有核心属性，例如总供应量、小数位数、是否有铸币权限的管理者 (mint authority)、
//      是否有冻结权限的管理者 (freeze authority) 等。
//    - 可以把它想象成这种代币的“模板”或“发行合约”。每种不同的代币（如USDC, RAY）都有其独立的 Mint 账户。
// 2. Token Account (代币账户/持有者账户):
//    - 用于存储特定用户（owner）拥有的特定种类代币（由其 Mint 账户地址标识）的数量。
//    - 类似于银行账户，但只针对一种特定的代币。它记录了代币余额、是否被冻结、是否有委托给其他地址进行操作等。
// 3. Multisig Account (多重签名账户):
//    - 允许多个签名者共同管理一个权限。例如，一个 Mint 账户的铸币权限可以被赋予一个 Multisig 账户，
//      这样就需要多个管理者共同签名才能铸造新的代币。
//    - 它记录了需要多少个签名 (M) 以及总共有多少个合法的签名者 (N) (即 M-of-N 签名)。
//
// 此模块能够处理标准 SPL Token 程序以及更新的 SPL Token 2022 程序（它引入了“扩展” (Extensions) 机制，
// 允许在 Mint 和 Token Account 中添加如利息、转账费、元数据指针等高级功能）。
// 代码会尝试反序列化账户的原始二进制数据，识别其类型（Mint, Account, or Multisig），
// 并将其转换为用户友好的、结构化的 Rust 类型 (UiMint, UiTokenAccount, UiMultisig)，这些类型之后可以方便地
// 序列化为 JSON 等格式供前端或工具展示。
//
// 此外，该文件还包含一些辅助函数，例如：
// - 获取所有已知的 SPL Token 程序 ID (spl_token::id() 和 spl_token_2022::id())。
// - 判断一个给定的程序 ID 是否属于已知的 SPL Token 程序。
// - 将代币的最小单位数量（u64 整数）根据其小数位数转换为用户可读的浮点数或字符串表示。
// - 从代币账户数据中安全地提取其对应的 Mint 账户地址。

// 重新导出一些在客户端类型库中定义的、与Token解析相关的UI结构体和工具函数。
// 这样做的好处是，使用 `account-decoder` 的其他 crate 可以直接通过 `account_decoder::parse_token::UiMint` 等方式访问这些类型，
// 而无需再单独依赖 `solana_account_decoder_client_types`。
pub use solana_account_decoder_client_types::token::{
    real_number_string, real_number_string_trimmed, TokenAccountType, UiAccountState, UiMint,
    UiMultisig, UiTokenAccount, UiTokenAmount,
    // `real_number_string`: 将 u64 金额和指定小数位数转换为完整的字符串表示 (e.g., "1.000000000")。
    // `real_number_string_trimmed`: 类似上面，但会去除尾部多余的零 (e.g., "1.000" -> "1")。
    // `TokenAccountType`: 枚举，表示解析后的 SPL Token 账户类型 (Mint, Account, Multisig, 或无法识别)。
    // `UiAccountState`: 枚举，表示代币账户的状态 (Uninitialized, Initialized, Frozen)。
    // `UiMint`: 用户友好的 Mint 账户数据结构。
    // `UiMultisig`: 用户友好的 Multisig 账户数据结构。
    // `UiTokenAccount`: 用户友好的代币账户数据结构。
    // `UiTokenAmount`: 包含代币数量的多种表示 (u64 原始值, f64 UI值, 字符串 UI值) 的结构体。
};
use {
    // 从当前 crate (solana-account-decoder) 导入。
    crate::{
        parse_account_data::{ // 从账户数据解析模块导入。
            ParsableAccount, ParseAccountError, SplTokenAdditionalData, SplTokenAdditionalDataV2,
            // `ParsableAccount`: 枚举，用于在解析失败时指明是哪种可解析账户类型。
            // `ParseAccountError`: 解析过程中可能发生的错误类型。
            // `SplTokenAdditionalData`, `SplTokenAdditionalDataV2`: 用于传递解析 SPL Token 所需的额外信息
            // (主要是代币的精度 decimals，以及 Token-2022 扩展可能需要的如生息配置等)。
        },
        parse_token_extension::parse_extension, // 导入用于解析 SPL Token 扩展的函数。
    },
    solana_program_option::COption, // `COption<T>` 是 Solana 程序中常用的一种可选类型，类似于 `Option<T>`，
                                    // 但在序列化时有特定行为 (例如，`COption::None` 通常序列化为全零)。
    solana_program_pack::Pack,      // `Pack` 是 Solana 程序定义的一个特性 (trait)，用于在固定大小的账户数据和 Rust 结构体之间进行序列化和反序列化。
    solana_pubkey::Pubkey,          // `Pubkey` 类型，表示 Solana 账户的公钥。
    spl_token_2022::{               // 从 SPL Token 2022 标准库导入。
        extension::{BaseStateWithExtensions, StateWithExtensions}, // 用于处理带有扩展的账户状态的结构体。
                                                                    // SPL Token 2022 允许在 Mint 和 Account 结构之后附加额外的扩展数据。
        generic_token_account::GenericTokenAccount, // 一个通用的代币账户接口，可能用于某些扩展。
        state::{Account, AccountState, Mint, Multisig}, // SPL Token 和 Token 2022 共用的核心状态结构定义：
                                                         // `Account`: 代币持有者账户的原始链上结构。
                                                         // `AccountState`: 代币账户的状态枚举 (Uninitialized, Initialized, Frozen)。
                                                         // `Mint`: 代币定义（铸币厂）账户的原始链上结构。
                                                         // `Multisig`: 多重签名账户的原始链上结构。
    },
    std::str::FromStr, // 从标准库导入 `FromStr` 特性，用于从字符串解析出特定类型的值 (例如 f64)。
};

// 函数 `spl_token_ids`：
// 功能：返回一个包含所有已知的 SPL Token 程序公钥ID的向量 (Vec)。
//       目前包括标准的 SPL Token 程序ID和 SPL Token 2022 程序ID。
//       这两个程序都用于管理代币，但 Token 2022 引入了更多高级功能（通过扩展）。
// 返回值：`Vec<Pubkey>`，包含两个公钥。
pub fn spl_token_ids() -> Vec<Pubkey> {
    vec![spl_token::id(), spl_token_2022::id()]
}

// 函数 `is_known_spl_token_id`：
// 功能：检查传入的 `program_id` 是否是已知的 SPL Token 程序ID之一。
// 参数：
//   - `program_id`: `&Pubkey`，要检查的程序公钥。
// 返回值：`bool`，如果是已知的 SPL Token 程序ID，则为 `true`，否则为 `false`。
pub fn is_known_spl_token_id(program_id: &Pubkey) -> bool {
    *program_id == spl_token::id() || *program_id == spl_token_2022::id()
}

// 函数 `parse_token` (已废弃):
// 这是旧版本的 SPL Token 解析函数。
// `#[deprecated(...)]` 宏标记此函数已不推荐使用，并建议使用 `parse_token_v3`。
// 它内部通过 `parse_token_v2` 调用了更新的逻辑。
// 参数：
//   - `data`: `&[u8]`，SPL Token 相关账户的原始二进制数据。
//   - `decimals`: `Option<u8>`，可选的代币小数位数。对于解析 Token Account 是必需的，
//                 因为需要用它来正确显示代币数量。对于 Mint Account，小数位数信息包含在自身数据中。
// 返回值：`Result<TokenAccountType, ParseAccountError>`，解析结果或错误。
#[deprecated(since = "2.0.0", note = "Use `parse_token_v3` instead")]
#[allow(deprecated)] // 允许在函数体内部调用其他已废弃的函数或使用已废弃的类型。
pub fn parse_token(
    data: &[u8],
    decimals: Option<u8>,
) -> Result<TokenAccountType, ParseAccountError> {
    // 将 Option<u8> 转换为 Option<SplTokenAdditionalData>，这是旧版附加数据结构。
    let additional_data = decimals.map(SplTokenAdditionalData::with_decimals);
    // 调用 v2 版本，传递对附加数据的引用。
    parse_token_v2(data, additional_data.as_ref())
}

// 函数 `parse_token_v2` (已废弃):
// SPL Token 解析函数的 V2 版本，同样已废弃，推荐使用 `parse_token_v3`。
// 它将 `SplTokenAdditionalData` 转换为 `SplTokenAdditionalDataV2`，然后调用 `parse_token_v3`。
// 参数：
//   - `data`: `&[u8]`，账户的原始二进制数据。
//   - `additional_data`: `Option<&SplTokenAdditionalData>`，可选的旧版附加数据引用。
// 返回值：`Result<TokenAccountType, ParseAccountError>`。
#[deprecated(since = "2.2.0", note = "Use `parse_token_v3` instead")]
pub fn parse_token_v2(
    data: &[u8],
    additional_data: Option<&SplTokenAdditionalData>,
) -> Result<TokenAccountType, ParseAccountError> {
    // 将 Option<&SplTokenAdditionalData> 转换为 Option<SplTokenAdditionalDataV2>。
    // `(*v).into()`: 首先解引用 `v` 得到 `SplTokenAdditionalData`，然后调用 `.into()`
    // 将其转换为 `SplTokenAdditionalDataV2` (因为 `SplTokenAdditionalDataV2` 实现了 `From<SplTokenAdditionalData>`)。
    let additional_data_v2 = additional_data.map(|v| (*v).into());
    // 调用 v3 版本，传递对新的附加数据结构 (v2) 的引用。
    parse_token_v3(data, additional_data_v2.as_ref())
}

// 函数 `parse_token_v3`：
// 功能：当前主要的 SPL Token (包括 Token-2022) 账户数据解析入口函数。
//       它会尝试将传入的原始字节数据 `data` 解析为 Token Account, Mint, 或 Multisig 中的一种。
//       这个函数能够处理带有扩展（Extensions）的 Token-2022 账户。
// 参数：
//   - `data`: `&[u8]`，SPL Token 相关账户的原始二进制数据。
//   - `additional_data`: `Option<&SplTokenAdditionalDataV2>`，可选的附加数据引用。
//     - 对于解析 Token Account，此参数**必须**提供，并且其 `decimals` 字段必须有效，用于正确转换代币数量。
//       Token-2022 的某些扩展（如生息代币）可能还需要此结构中的其他字段（如 `interest_bearing_config`）。
//     - 对于解析 Mint 或 Multisig 账户，此参数通常为 `None` 或被忽略，因为这些账户类型自身包含了足够的信息
//       (例如 Mint 账户自己就存储了 `decimals`)。
// 返回值：
//   - `Result<TokenAccountType, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(TokenAccountType)`，其中 `TokenAccountType` 是一个枚举，
//       包含了具体账户类型 (`Account`, `Mint`, `Multisig`) 及其用户友好的数据结构 (`UiTokenAccount`, `UiMint`, `UiMultisig`)。
//     - 如果数据无法被识别为任何一种 SPL Token 账户类型，或者在解析过程中出错（例如，解析 Token Account 时缺少 `decimals`），
//       则返回 `Err(ParseAccountError)`。
// 设计思路：
//   1. 首先，尝试将 `data` 按带有扩展的 Token Account (`StateWithExtensions::<Account>`) 进行解包 (unpack)。
//      `StateWithExtensions::unpack(data)` 会尝试反序列化基础的 `Account` 结构，并识别出任何附加的扩展数据。
//      - 如果成功解包为 Token Account:
//        a. 检查是否提供了 `additional_data`。对于 Token Account，这是必需的（至少需要 `decimals`）。
//           如果 `additional_data` 为 `None`，则返回 `ParseAccountError::AdditionalDataMissing` 错误。
//           `.as_ref()` 将 `Option<&T>` 转换为 `Option<&T>` (这里 T 是 `SplTokenAdditionalDataV2`)，
//           然后 `ok_or_else(...)` 在 `None` 的情况下构造并返回错误。
//        b. 获取账户中存在的所有扩展类型列表 (`account.get_extension_types()`)。
//        c. 遍历这些扩展类型，对每种类型调用 `parse_extension::<Account>(...)` 来解析该扩展的具体内容，
//           并将其转换为对应的 UI 扩展结构。收集所有解析后的 UI 扩展到一个 Vec 中。
//        d. 从基础的 `account.base` 结构中提取字段 (如 `mint`, `owner`, `amount`, `state`, `delegate` 等)，
//           并将它们转换为用户友好的格式 (例如，`Pubkey` 转为 `String`，`u64` 金额结合 `decimals` 转为 `UiTokenAmount`)。
//           - `COption<Pubkey>` 类型的字段（如 `delegate`, `close_authority`）需要 `match` 来处理 `Some` 和 `None` 的情况。
//           - `is_native` 字段也需要特殊处理，它是一个 `COption<u64>`，表示如果是原生 SOL 包装代币，这里会存储租金豁免的 lamports 数量。
//        e. 构建一个 `UiTokenAccount` 结构，填充所有这些转换后的字段和解析出的 UI 扩展。
//        f. 将 `UiTokenAccount` 包装在 `TokenAccountType::Account` 中并作为 `Ok` 结果返回。
//   2. 如果上一步解包 Token Account 失败，则接着尝试将 `data` 按带有扩展的 Mint Account (`StateWithExtensions::<Mint>`) 进行解包。
//      - 如果成功解包为 Mint Account:
//        a. 类似地，获取并解析所有存在的扩展，得到一个 UI 扩展列表。
//        b. 从基础的 `mint.base` 结构中提取字段 (如 `mint_authority`, `supply`, `decimals`, `is_initialized`, `freeze_authority`)，
//           并转换为用户友好的格式。
//        c. 构建一个 `UiMint` 结构，填充这些字段和 UI 扩展。
//        d. 将 `UiMint` 包装在 `TokenAccountType::Mint` 中并作为 `Ok` 结果返回。
//   3. 如果以上两种解包都失败，则检查 `data` 的长度是否等于标准 Multisig 账户的固定长度 (`Multisig::get_packed_len()`)。
//      - 如果长度匹配:
//        a. 尝试使用 `Multisig::unpack(data)` 将数据解包为 `Multisig` 结构。
//           如果解包失败，返回 `ParseAccountError::AccountNotParsable(ParsableAccount::SplToken)`。
//        b. 从 `multisig` 结构中提取字段 (`m` - 所需签名数, `n` - 总签名数, `is_initialized`, `signers` 列表)。
//        c. 转换 `signers` 列表，将非默认的 `Pubkey` (即非全零的公钥) 转换为字符串。
//        d. 构建一个 `UiMultisig` 结构。
//        e. 将 `UiMultisig` 包装在 `TokenAccountType::Multisig` 中并作为 `Ok` 结果返回。
//   4. 如果以上所有尝试都失败，说明 `data` 不是任何已知的 SPL Token 账户类型，
//      则返回 `Err(ParseAccountError::AccountNotParsable(ParsableAccount::SplToken))`。
pub fn parse_token_v3(
    data: &[u8],
    additional_data: Option<&SplTokenAdditionalDataV2>, // Token Account 解析时需要此参数提供 decimals 等信息
) -> Result<TokenAccountType, ParseAccountError> {
    // 尝试解析为 Token Account (支持 Token-2022 扩展)
    if let Ok(account_with_extensions) = StateWithExtensions::<Account>::unpack(data) {
        // 对于 Token Account，必须提供 additional_data (至少包含 decimals)
        let required_additional_data = additional_data.as_ref().ok_or_else(|| {
            ParseAccountError::AdditionalDataMissing(
                "no mint_decimals provided to parse spl-token account".to_string(),
            )
        })?;

        // 获取并解析所有存在的扩展
        let extension_types = account_with_extensions.get_extension_types().unwrap_or_default();
        let ui_extensions = extension_types
            .iter()
            .map(|extension_type| parse_extension::<Account>(extension_type, &account_with_extensions))
            .collect();

        // 从基础账户数据 (account_with_extensions.base) 中提取信息并转换为 UI 格式
        let base_account = &account_with_extensions.base;
        return Ok(TokenAccountType::Account(UiTokenAccount {
            mint: base_account.mint.to_string(),
            owner: base_account.owner.to_string(),
            token_amount: token_amount_to_ui_amount_v3(base_account.amount, required_additional_data),
            delegate: match base_account.delegate { // COption<Pubkey> -> Option<String>
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            state: convert_account_state(base_account.state), // AccountState -> UiAccountState
            is_native: base_account.is_native(), // is_native() 返回 bool
            rent_exempt_reserve: match base_account.is_native { // COption<u64> -> Option<UiTokenAmount>
                COption::Some(reserve) => {
                    Some(token_amount_to_ui_amount_v3(reserve, required_additional_data))
                }
                COption::None => None,
            },
            delegated_amount: if base_account.delegate.is_none() { // 只有当存在委托时，委托金额才有意义
                None
            } else {
                Some(token_amount_to_ui_amount_v3(
                    base_account.delegated_amount,
                    required_additional_data,
                ))
            },
            close_authority: match base_account.close_authority { // COption<Pubkey> -> Option<String>
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            extensions: ui_extensions, // 已解析的 UI 扩展列表
        }));
    }

    // 如果不是 Token Account，尝试解析为 Mint Account (支持 Token-2022 扩展)
    if let Ok(mint_with_extensions) = StateWithExtensions::<Mint>::unpack(data) {
        // 获取并解析所有存在的扩展
        let extension_types = mint_with_extensions.get_extension_types().unwrap_or_default();
        let ui_extensions = extension_types
            .iter()
            .map(|extension_type| parse_extension::<Mint>(extension_type, &mint_with_extensions))
            .collect();

        // 从基础 Mint 数据 (mint_with_extensions.base) 中提取信息并转换为 UI 格式
        let base_mint = &mint_with_extensions.base;
        return Ok(TokenAccountType::Mint(UiMint {
            mint_authority: match base_mint.mint_authority { // COption<Pubkey> -> Option<String>
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            supply: base_mint.supply.to_string(), // u64 -> String
            decimals: base_mint.decimals,         // u8, 直接使用
            is_initialized: base_mint.is_initialized, // bool, 直接使用
            freeze_authority: match base_mint.freeze_authority { // COption<Pubkey> -> Option<String>
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            extensions: ui_extensions, // 已解析的 UI 扩展列表
        }));
    }

    // 如果也不是 Mint Account，尝试解析为 Multisig Account (多重签名账户通常没有扩展)
    // Multisig 账户有一个固定的大小。
    if data.len() == Multisig::get_packed_len() {
        // 使用 `Multisig::unpack` 反序列化。
        let multisig = Multisig::unpack(data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::SplToken))?; // 如果解包失败，返回错误

        Ok(TokenAccountType::Multisig(UiMultisig {
            num_required_signers: multisig.m, // 需要的签名数 (M)
            num_valid_signers: multisig.n,    // 总的有效签名者数量 (N)
            is_initialized: multisig.is_initialized, // 是否已初始化
            signers: multisig // `multisig.signers` 是一个 `[Pubkey; 11]` 数组
                .signers
                .iter() // 迭代签名者公钥数组
                .filter_map(|pubkey| { // 过滤掉默认的 (全零) 公钥，因为数组可能未完全填满
                    if pubkey != &Pubkey::default() {
                        Some(pubkey.to_string()) // 将有效的公钥转换为字符串
                    } else {
                        None // 忽略默认公钥
                    }
                })
                .collect(), // 收集到 `Vec<String>`
        }))
    } else {
        // 如果数据长度不匹配 Multisig，并且之前的尝试都失败了，则认为无法解析。
        Err(ParseAccountError::AccountNotParsable(
            ParsableAccount::SplToken, // 指明是 SPL Token 类型的账户解析失败
        ))
    }
}

// 函数 `convert_account_state`：
// 功能：将原始的链上 `AccountState` 枚举转换为用户友好的 `UiAccountState` 枚举。
// 参数：
//   - `state`: `AccountState`，从链上数据反序列化得到的原始账户状态。
// 返回值：`UiAccountState`，对应的 UI 版本状态。
// 设计思路：简单的 `match` 表达式进行一对一的转换。
pub fn convert_account_state(state: AccountState) -> UiAccountState {
    match state {
        AccountState::Uninitialized => UiAccountState::Uninitialized, // 未初始化
        AccountState::Initialized => UiAccountState::Initialized,   // 已初始化 (可用)
        AccountState::Frozen => UiAccountState::Frozen,             // 已冻结 (不可转账等操作)
    }
}

// 函数 `token_amount_to_ui_amount` (已废弃):
// 旧版本的将代币原子单位数量转换为 UI 友好结构 (`UiTokenAmount`) 的函数。
// `#[deprecated(...)]` 宏标记此函数已不推荐使用，建议使用 `token_amount_to_ui_amount_v3`。
// 参数：
//   - `amount`: `u64`，代币的原子单位数量 (例如 lamports)。
//   - `decimals`: `u8`，该代币的小数位数。
// 返回值：`UiTokenAmount`。
#[deprecated(since = "2.0.0", note = "Use `token_amount_to_ui_amount_v3` instead")]
#[allow(deprecated)] // 允许函数体内部调用其他已废弃的函数。
pub fn token_amount_to_ui_amount(amount: u64, decimals: u8) -> UiTokenAmount {
    // 内部调用 v2 版本，构造一个临时的旧版附加数据结构。
    token_amount_to_ui_amount_v2(amount, &SplTokenAdditionalData::with_decimals(decimals))
}

// 函数 `token_amount_to_ui_amount_v2` (已废弃):
// 将代币原子单位数量转换为 `UiTokenAmount` 的 V2 版本，同样已废弃。
// 它将旧版的 `SplTokenAdditionalData` 转换为 `SplTokenAdditionalDataV2`，然后调用 `v3` 版本。
// 参数：
//   - `amount`: `u64`，代币的原子单位数量。
//   - `additional_data`: `&SplTokenAdditionalData`，包含小数位数等信息的旧版附加数据。
// 返回值：`UiTokenAmount`。
#[deprecated(since = "2.2.0", note = "Use `token_amount_to_ui_amount_v3` instead")]
pub fn token_amount_to_ui_amount_v2(
    amount: u64,
    additional_data: &SplTokenAdditionalData,
) -> UiTokenAmount {
    // 将旧的附加数据转换为新的附加数据格式 (v2)，然后调用 v3 函数。
    token_amount_to_ui_amount_v3(amount, &(*additional_data).into())
}

// 函数 `token_amount_to_ui_amount_v3`：
// 功能：将代币的原子单位数量（u64）根据其小数位数和可能的 Token-2022 扩展（如生息、UI金额缩放）
//       转换为用户友好的 `UiTokenAmount` 结构。
//       `UiTokenAmount` 包含三种表示：可选的 f64 数值 (`ui_amount`)，原始 u64 数量的字符串 (`amount`)，
//       以及最终用于UI显示的字符串 (`ui_amount_string`)。
// 参数：
//   - `amount`: `u64`，代币的原子单位数量。
//   - `additional_data`: `&SplTokenAdditionalDataV2`，包含代币小数位数 (`decimals`) 以及可选的
//     `interest_bearing_config` (生息配置) 或 `scaled_ui_amount_config` (UI金额缩放配置)。
// 返回值：`UiTokenAmount`。
// 设计思路：
//   1. 从 `additional_data` 中获取小数位数 `decimals`。
//   2. 检查 `additional_data` 中是否存在并优先使用 Token-2022 扩展配置：
//      a. 如果存在 `interest_bearing_config` (生息代币配置):
//         - 使用 `interest_bearing_config.amount_to_ui_amount(amount, decimals, unix_timestamp)`
//           计算考虑了利息累积后的 UI 显示字符串。这个方法会返回 `Option<String>`。
//         - 如果计算成功得到 `Some(str_val)`，则尝试将 `str_val` 解析为 `f64` 作为 `ui_amount`。
//           如果解析 `f64` 失败，`ui_amount` 为 `None`。
//         - `ui_amount_string` 使用计算得到的字符串，如果计算失败则为空字符串。
//      b. 否则，如果存在 `scaled_ui_amount_config` (UI金额缩放配置):
//         - 类似地，使用 `scaled_ui_amount_config.amount_to_ui_amount(...)` 计算 UI 显示字符串。
//         - 尝试将结果字符串解析为 `f64`。
//      c. 如果以上两种扩展配置都不存在（即标准 SPL Token 或无特殊 UI 处理的 Token-2022）:
//         - 计算 `ui_amount` (f64): `amount / (10^decimals)`。
//           使用 `checked_pow` 防止 `10^decimals` 溢出。如果 `decimals` 过大导致 `10^decimals` 无法计算，`ui_amount` 会是 `None`。
//         - 计算 `ui_amount_string`: 使用 `real_number_string_trimmed(amount, decimals)` 得到修整后的字符串表示。
//   3. 构建并返回 `UiTokenAmount` 结构，包含计算得到的 `ui_amount` (Option<f64>)，
//      原始的 `decimals` (u8)，原始 `amount` 的字符串形式，以及最终的 `ui_amount_string`。
pub fn token_amount_to_ui_amount_v3(
    amount: u64, // 原始的 token 数量 (最小单位)
    additional_data: &SplTokenAdditionalDataV2, // 包含 decimals 和可能的扩展配置
) -> UiTokenAmount {
    let decimals = additional_data.decimals; // 获取小数位数
    let (ui_amount_opt_f64, ui_amount_str): (Option<f64>, String) = // 元组，存储计算出的 Option<f64> 和 String
        // 检查是否有生息代币配置
        if let Some((interest_bearing_config, unix_timestamp)) =
            additional_data.interest_bearing_config
        {
            // 使用生息配置计算 UI 字符串
            let calc_ui_amount_string =
                interest_bearing_config.amount_to_ui_amount(amount, decimals, unix_timestamp);
            (
                // 尝试将计算出的字符串解析为 f64
                calc_ui_amount_string.as_ref().and_then(|s| f64::from_str(s).ok()),
                // 如果字符串计算失败，则使用空字符串
                calc_ui_amount_string.unwrap_or_else(|| "".to_string()),
            )
        }
        // 否则，检查是否有 UI 金额缩放配置
        else if let Some((scaled_ui_amount_config, unix_timestamp)) =
            additional_data.scaled_ui_amount_config
        {
            // 使用缩放配置计算 UI 字符串
            let calc_ui_amount_string =
                scaled_ui_amount_config.amount_to_ui_amount(amount, decimals, unix_timestamp);
            (
                // 尝试解析为 f64
                calc_ui_amount_string.as_ref().and_then(|s| f64::from_str(s).ok()),
                calc_ui_amount_string.unwrap_or_else(|| "".to_string()),
            )
        }
        // 如果没有特殊扩展，则按标准方式计算
        else {
            // 计算 f64 格式的 UI 金额: amount / (10^decimals)
            // `checked_pow` 用于安全地计算幂，防止溢出。
            let f64_ui_amount = 10_usize
                .checked_pow(decimals as u32) // 计算 10 的 decimals 次方
                .map(|divisor| amount as f64 / divisor as f64); // 如果成功，进行除法
            // 使用 `real_number_string_trimmed` 获取修整后的字符串表示
            (f64_ui_amount, real_number_string_trimmed(amount, decimals))
        };

    // 构建并返回 UiTokenAmount 结构体
    UiTokenAmount {
        ui_amount: ui_amount_opt_f64,        // 计算得到的 Option<f64> 金额
        decimals,                            // 代币的小数位数
        amount: amount.to_string(),          // 原始 u64 金额的字符串形式
        ui_amount_string: ui_amount_str,     // 最终用于 UI 显示的字符串形式
    }
}

// 函数 `get_token_account_mint`：
// 功能：安全地从一个可能是 SPL Token Account 的账户数据中提取其关联的 Mint 账户公钥。
// 参数：
//   - `data`: `&[u8]`，账户的原始二进制数据。
// 返回值：`Option<Pubkey>`:
//   - 如果 `data` 是一个有效的 SPL Token Account 数据并且能够成功提取 Mint 公钥，则返回 `Some(mint_pubkey)`。
//   - 否则（例如数据太短、不是有效的 Account 结构、或 Mint 公钥部分无效），返回 `None`。
// 设计思路：
//   1. `Account::valid_account_data(data)`: 首先检查传入的 `data` 是否至少满足 SPL Token Account 数据的最小长度要求。
//      这是一个快速的初步校验。
//   2. `.then(|| ...)`: 如果上一步检查通过 (返回 `true`)，则执行闭包内的逻辑。
//      闭包尝试从 `data` 的开头提取 Mint 公钥。SPL Token Account 结构中，`mint` 字段（一个 `Pubkey`）位于数据的最开始32个字节。
//      - `data.get(..32)?`: 尝试获取 `data` 的前32个字节的切片。`?` 用于错误传播：如果 `data` 长度不足32字节，`get` 返回 `None`，
//        则整个闭包的结果也是 `None` (通过 `?` 传递出去)。
//      - `Pubkey::try_from(...)`: 尝试将这32字节的切片转换为一个 `Pubkey`。如果切片不是有效的公钥表示，`try_from` 返回 `Err`。
//      - `.ok()`: 将 `Result<Pubkey, _>` 转换为 `Option<Pubkey>`，忽略错误细节。
//      所以，闭包的结果是 `Option<Pubkey>`。
//   3. `.flatten()`: 由于 `.then(|| ...)` 返回的是 `Option<Option<Pubkey>>` (因为闭包自身返回 `Option<Pubkey>`)，
//      使用 `.flatten()` 将其转换为 `Option<Pubkey>`。如果外部的 `Option` 是 `None`，或内部的 `Option` 是 `None`，
//      最终结果都是 `None`。只有当两层都是 `Some` 时，才得到 `Some(mint_pubkey)`。
pub fn get_token_account_mint(data: &[u8]) -> Option<Pubkey> {
    // 1. 初步校验数据是否可能是有效的 Token Account 数据 (通常检查长度)
    Account::valid_account_data(data)
        // 2. 如果校验通过，则尝试提取 Mint Pubkey
        .then(|| { // `.then` 在前一个表达式为 true 时执行闭包
            // Mint Pubkey 位于 Token Account 数据的前32个字节
            let mint_bytes_slice = data.get(..32)?; // 安全地获取前32字节，如果不足则返回 None (通过 `?`)
            Pubkey::try_from(mint_bytes_slice).ok() // 尝试将字节切片转换为 Pubkey，并将 Result 转为 Option
        })
        // 3. 扁平化 Option<Option<Pubkey>> 为 Option<Pubkey>
        .flatten()
}

// 测试模块
#[cfg(test)]
mod test {
    // 导入父模块的所有项 (`super::*`) 和测试中需要的特定项。
    use {
        super::*, // 导入当前文件 (parse_token.rs) 中的所有公开项。
        // 从当前 crate (solana-account-decoder) 的 token 扩展解析模块导入 UI 结构体。
        crate::parse_token_extension::{UiMemoTransfer, UiMintCloseAuthority},
        // 从客户端类型库导入 UI 扩展枚举。
        solana_account_decoder_client_types::token::UiExtension,
        // 从 spl_pod (Plain Old Data) 库导入用于可选非零公钥的类型，常用于扩展中。
        spl_pod::optional_keys::OptionalNonZeroPubkey,
        // 从 SPL Token 2022 库导入扩展相关的核心类型。
        spl_token_2022::extension::{
            immutable_owner::ImmutableOwner, // 不可变所有者扩展
            interest_bearing_mint::InterestBearingConfig, // 生息代币配置扩展
            memo_transfer::MemoTransfer,     // 转账备注扩展
            mint_close_authority::MintCloseAuthority, // Mint 关闭权限扩展
            scaled_ui_amount::ScaledUiAmountConfig, // UI 金额缩放配置扩展
            BaseStateWithExtensionsMut,      // 用于可变地访问带扩展的基础状态
            ExtensionType,                   // 枚举，表示所有支持的扩展类型
            StateWithExtensionsMut,          // 用于可变地访问带扩展的账户状态 (包括基础状态和扩展数据)
        },
    };

    // 定义一个常量，表示一年中的大约秒数，用于生息代币的测试计算。
    // 60秒 * 60分 * 24小时 * 365.24天 (考虑到闰年)
    const INT_SECONDS_PER_YEAR: i64 = 60 * 60 * 24 * 36524 / 100; // 注意: 源码中是 6*6*24*36524，这里修正为 60*60

    // 测试函数 `test_parse_token`
    // `#[test]` 宏标记这是一个单元测试函数。
    #[test]
    fn test_parse_token() {
        // --- 测试 Token Account 的解析 ---
        let mint_pubkey = Pubkey::new_from_array([2; 32]); // 创建一个模拟的 Mint 地址
        let owner_pubkey = Pubkey::new_from_array([3; 32]); // 创建一个模拟的 Owner 地址
        let mut account_data = vec![0; Account::get_packed_len()]; // 创建一个足够存储标准 Account 结构的空间

        // 手动填充 Account 结构的数据
        // `Account::unpack_unchecked` 从原始字节反序列化，不进行严格检查 (测试中常用)
        let mut account = Account::unpack_unchecked(&account_data).unwrap();
        account.mint = mint_pubkey;
        account.owner = owner_pubkey;
        account.amount = 42; // 代币数量为 42 (原子单位)
        account.state = AccountState::Initialized; // 状态为已初始化
        account.is_native = COption::None; // 不是原生 SOL 包装代币
        account.close_authority = COption::Some(owner_pubkey); // 设置关闭账户的权限
        Account::pack(account, &mut account_data).unwrap(); // 将修改后的 Account 结构序列化回字节数据

        // 断言1: 如果不提供 additional_data (包含 decimals)，解析应该失败
        assert!(parse_token_v3(&account_data, None).is_err());
        // 断言2: 提供 decimals 后，解析应该成功并返回预期的 UiTokenAccount
        assert_eq!(
            parse_token_v3(
                &account_data,
                Some(&SplTokenAdditionalDataV2::with_decimals(2)) // 提供小数位数为 2
            )
            .unwrap(),
            TokenAccountType::Account(UiTokenAccount { // 预期的解析结果
                mint: mint_pubkey.to_string(),
                owner: owner_pubkey.to_string(),
                token_amount: UiTokenAmount {
                    ui_amount: Some(0.42), // 42 / 10^2 = 0.42
                    decimals: 2,
                    amount: "42".to_string(),
                    ui_amount_string: "0.42".to_string()
                },
                delegate: None, // 无委托
                state: UiAccountState::Initialized,
                is_native: false,
                rent_exempt_reserve: None, // 非原生代币，此项为 None
                delegated_amount: None,    // 无委托金额
                close_authority: Some(owner_pubkey.to_string()),
                extensions: vec![], // 标准 Token Account，无扩展
            }),
        );

        // --- 测试 Mint Account 的解析 ---
        let mut mint_data = vec![0; Mint::get_packed_len()]; // 为 Mint 结构分配空间
        // 手动填充 Mint 结构的数据
        let mut mint = Mint::unpack_unchecked(&mint_data).unwrap();
        mint.mint_authority = COption::Some(owner_pubkey); // 铸币权限
        mint.supply = 42; // 总供应量
        mint.decimals = 3; // 小数位数
        mint.is_initialized = true; // 已初始化
        mint.freeze_authority = COption::Some(owner_pubkey); // 冻结权限
        Mint::pack(mint, &mut mint_data).unwrap(); // 序列化回字节

        // 解析 Mint 账户 (通常不需要 additional_data)
        assert_eq!(
            parse_token_v3(&mint_data, None).unwrap(),
            TokenAccountType::Mint(UiMint { // 预期的解析结果
                mint_authority: Some(owner_pubkey.to_string()),
                supply: "42".to_string(),
                decimals: 3,
                is_initialized: true,
                freeze_authority: Some(owner_pubkey.to_string()),
                extensions: vec![], // 标准 Mint Account，无扩展
            }),
        );

        // --- 测试 Multisig Account 的解析 ---
        let signer1 = Pubkey::new_from_array([1; 32]);
        let signer2 = Pubkey::new_from_array([2; 32]);
        let signer3 = Pubkey::new_from_array([3; 32]);
        let mut multisig_data = vec![0; Multisig::get_packed_len()]; // 为 Multisig 结构分配空间
        let mut signers_array = [Pubkey::default(); 11]; // Multisig 最多支持11个签名者
        signers_array[0] = signer1;
        signers_array[1] = signer2;
        signers_array[2] = signer3;
        // 手动填充 Multisig 结构数据
        let mut multisig = Multisig::unpack_unchecked(&multisig_data).unwrap();
        multisig.m = 2; // 需要 2 个签名 (M)
        multisig.n = 3; // 总共 3 个有效签名者 (N)
        multisig.is_initialized = true;
        multisig.signers = signers_array; // 设置签名者列表
        Multisig::pack(multisig, &mut multisig_data).unwrap(); // 序列化

        // 解析 Multisig 账户
        assert_eq!(
            parse_token_v3(&multisig_data, None).unwrap(),
            TokenAccountType::Multisig(UiMultisig { // 预期的解析结果
                num_required_signers: 2,
                num_valid_signers: 3,
                is_initialized: true,
                signers: vec![ // 预期只包含有效的签名者
                    signer1.to_string(),
                    signer2.to_string(),
                    signer3.to_string()
                ],
            }),
        );

        // --- 测试无效数据 ---
        let bad_data = vec![0; 4]; // 明显过短且无效的数据
        assert!(parse_token_v3(&bad_data, None).is_err()); // 解析应失败
    }

    // 测试函数 `test_get_token_account_mint`
    // 用于测试从 Token Account 数据中提取 Mint 地址的功能。
    #[test]
    fn test_get_token_account_mint() {
        let mint_pubkey_arr = [2u8; 32]; // 定义 mint pubkey 的字节数组
        let mint_pubkey = Pubkey::new_from_array(mint_pubkey_arr);
        let mut account_data = vec![0; Account::get_packed_len()];
        let mut account = Account::unpack_unchecked(&account_data).unwrap();
        account.mint = mint_pubkey; // 设置 mint 地址
        account.state = AccountState::Initialized; // 必须是 Initialized 状态才算有效
        Account::pack(account, &mut account_data).unwrap();

        let expected_mint_pubkey = Pubkey::from(mint_pubkey_arr); // 从相同字节数组创建预期的 Pubkey
        assert_eq!(
            get_token_account_mint(&account_data), // 调用被测函数
            Some(expected_mint_pubkey)             // 结果应为 Some(expected_mint_pubkey)
        );

        // 测试无效数据 (例如，未初始化的账户数据，或者长度不足)
        let mut invalid_account_data = vec![0; Account::get_packed_len()];
        let mut invalid_account = Account::unpack_unchecked(&invalid_account_data).unwrap();
        invalid_account.state = AccountState::Uninitialized; // 设置为未初始化
        Account::pack(invalid_account, &mut invalid_account_data).unwrap();
        assert_eq!(get_token_account_mint(&invalid_account_data), None); // 对于未初始化的账户，应返回 None

        let short_data = vec![0; 31]; // 长度不足32字节
        assert_eq!(get_token_account_mint(&short_data), None);
    }

    // 测试函数 `test_ui_token_amount_real_string`
    // 测试将 u64 金额和 decimals 转换为 UI 字符串的逻辑 (不涉及扩展)。
    #[test]
    fn test_ui_token_amount_real_string() {
        // 测试 decimals = 0
        assert_eq!(&real_number_string(1, 0), "1");
        assert_eq!(&real_number_string_trimmed(1, 0), "1");
        let token_amount_1_0 =
            token_amount_to_ui_amount_v3(1, &SplTokenAdditionalDataV2::with_decimals(0));
        assert_eq!(token_amount_1_0.ui_amount_string, "1");
        assert_eq!(token_amount_1_0.ui_amount, Some(1.0));

        assert_eq!(&real_number_string(10, 0), "10");
        assert_eq!(&real_number_string_trimmed(10, 0), "10");
        let token_amount_10_0 =
            token_amount_to_ui_amount_v3(10, &SplTokenAdditionalDataV2::with_decimals(0));
        assert_eq!(token_amount_10_0.ui_amount_string, "10");
        assert_eq!(token_amount_10_0.ui_amount, Some(10.0));

        // 测试 decimals = 9
        assert_eq!(&real_number_string(1, 9), "0.000000001");
        assert_eq!(&real_number_string_trimmed(1, 9), "0.000000001"); // trim 对此无影响
        let token_amount_1_9 =
            token_amount_to_ui_amount_v3(1, &SplTokenAdditionalDataV2::with_decimals(9));
        assert_eq!(token_amount_1_9.ui_amount_string, "0.000000001");
        assert_eq!(token_amount_1_9.ui_amount, Some(0.000000001));

        assert_eq!(&real_number_string(1_000_000_000, 9), "1.000000000");
        assert_eq!(&real_number_string_trimmed(1_000_000_000, 9), "1"); // trim 生效
        let token_amount_1b_9 = token_amount_to_ui_amount_v3(
            1_000_000_000,
            &SplTokenAdditionalDataV2::with_decimals(9),
        );
        assert_eq!(token_amount_1b_9.ui_amount_string, "1");
        assert_eq!(token_amount_1b_9.ui_amount, Some(1.0));

        // 测试 decimals = 3, amount 较大
        assert_eq!(&real_number_string(1_234_567_890, 3), "1234567.890");
        assert_eq!(&real_number_string_trimmed(1_234_567_890, 3), "1234567.89"); // trim 生效
        let token_amount_large_3 = token_amount_to_ui_amount_v3(
            1_234_567_890,
            &SplTokenAdditionalDataV2::with_decimals(3),
        );
        assert_eq!(token_amount_large_3.ui_amount_string, "1234567.89");
        assert_eq!(token_amount_large_3.ui_amount, Some(1234567.89));

        // 测试 decimals 较大 (例如 20 或 25)，可能导致 f64 精度问题
        // `real_number_string` 会完整显示，但 `real_number_string_trimmed` 可能会截断或表现不同
        // `ui_amount` (f64) 在这种情况下可能变为 None，因为超出 f64 的精确表示范围或计算结果非常小
        assert_eq!(
            &real_number_string(1_234_567_890, 25),
            "0.0000000000000001234567890" // 25位小数
        );
        // `real_number_string_trimmed` 的行为取决于其内部的修整逻辑，对于非常小的小数，可能保留一定位数或科学计数法
        // 此处预期是截断到一定精度
        assert_eq!(
            &real_number_string_trimmed(1_234_567_890, 25),
            "0.000000000000000123456789" // 假设截断到非零部分后几位
        );
        let token_amount_large_decimals = token_amount_to_ui_amount_v3(
            1_234_567_890,
            &SplTokenAdditionalDataV2::with_decimals(20), // 使用20位小数
        );
        assert_eq!(
            token_amount_large_decimals.ui_amount_string,
            real_number_string_trimmed(1_234_567_890, 20)
        );
        // 当小数位数过多，amount 相对较小时，f64 可能无法精确表示，或者结果为0.0，或者None
        // 此处预期为 None，因为 `10_usize.checked_pow(20)` 很大，除法结果可能小于 f64 能表示的最小正数，或实现中判定为 None
        assert_eq!(token_amount_large_decimals.ui_amount, None);
    }

    // 测试函数 `test_ui_token_amount_with_interest`
    // 测试生息代币 (Interest-Bearing Token) 的 UI 金额计算。
    #[test]
    fn test_ui_token_amount_with_interest() {
        // 场景1: 固定 5% 年利率
        let config = InterestBearingConfig {
            initialization_timestamp: 0.into(), // 初始化时间戳为0
            pre_update_average_rate: 500.into(), // 初始平均利率 (基点, 500 = 5%)
            last_update_timestamp: INT_SECONDS_PER_YEAR.into(), // 上次更新时间为1年后 (为了简化，假设利率在这一年内固定)
            current_rate: 500.into(), // 当前利率 (基点)
            ..Default::default() // 其他字段使用默认值
        };
        let additional_data_interest = SplTokenAdditionalDataV2 {
            decimals: 18, // 假设18位小数
            interest_bearing_config: Some((config, INT_SECONDS_PER_YEAR)), // 提供生息配置和当前时间戳 (1年后)
            ..Default::default()
        };
        const ONE_ATOM_D18: u64 = 1_000_000_000_000_000_000; // 1个完整代币 (10^18 原子单位)
        const TEN_ATOM_D18: u64 = 10_000_000_000_000_000_000; // 10个完整代币

        let token_amount_one = token_amount_to_ui_amount_v3(ONE_ATOM_D18, &additional_data_interest);
        // 预期结果约为 1 * (1 + 0.05)^(1年/1年) = 1.05。实际计算会更复杂，涉及连续复利或特定算法。
        // 此处我们只验证字符串的开头和 f64 值的近似相等。
        assert!(token_amount_one
            .ui_amount_string
            .starts_with("1.05127")); // 实际的计算结果，由 `amount_to_ui_amount` 方法提供
        assert!((token_amount_one.ui_amount.unwrap() - 1.0512710963760241f64).abs() < f64::EPSILON);

        let token_amount_ten = token_amount_to_ui_amount_v3(TEN_ATOM_D18, &additional_data_interest);
        assert!(token_amount_ten
            .ui_amount_string
            .starts_with("10.51271"));
        assert!((token_amount_ten.ui_amount.unwrap() - 10.512710963760242f64).abs() < f64::EPSILON);

        // 场景2: 利率和时间跨度极大，可能导致数值溢出或显示为 "inf" (无穷大)
        let config_huge_rate = InterestBearingConfig {
            initialization_timestamp: 0.into(),
            pre_update_average_rate: 32767.into(), // 极大的利率 (327.67%)
            last_update_timestamp: 0.into(),
            current_rate: 32767.into(),
            ..Default::default()
        };
        let additional_data_huge = SplTokenAdditionalDataV2 {
            decimals: 0, // 0位小数，方便观察大数
            interest_bearing_config: Some((config_huge_rate, INT_SECONDS_PER_YEAR * 1_000)), // 1000年
            ..Default::default()
        };
        let token_amount_huge = token_amount_to_ui_amount_v3(u64::MAX, &additional_data_huge); // 最大u64金额
        assert_eq!(token_amount_huge.ui_amount, Some(f64::INFINITY)); // f64 值应为无穷大
        assert_eq!(token_amount_huge.ui_amount_string, "inf");      // 字符串表示为 "inf"
    }

    // 测试函数 `test_ui_token_amount_with_multiplier`
    // 测试 UI 金额缩放 (Scaled UI Amount) 扩展的计算。
    #[test]
    fn test_ui_token_amount_with_multiplier() {
        // 场景1: 2倍缩放因子
        let config_scale = ScaledUiAmountConfig {
            new_multiplier: 2f64.into(), // 缩放因子为 2.0
            ..Default::default()
        };
        let additional_data_scale = SplTokenAdditionalDataV2 {
            decimals: 18, // 18位小数
            scaled_ui_amount_config: Some((config_scale, 0)), // 提供缩放配置，时间戳通常不影响此扩展
            ..Default::default()
        };
        const ONE_ATOM_D18_SCALE: u64 = 1_000_000_000_000_000_000; // 1个代币
        const TEN_ATOM_D18_SCALE: u64 = 10_000_000_000_000_000_000; // 10个代币

        let token_amount_one_scaled = token_amount_to_ui_amount_v3(ONE_ATOM_D18_SCALE, &additional_data_scale);
        // 1.0 * 2.0 = 2.0
        assert_eq!(token_amount_one_scaled.ui_amount_string, "2"); // 字符串应为 "2"
        assert!((token_amount_one_scaled.ui_amount.unwrap() - 2.0).abs() < f64::EPSILON);

        let token_amount_ten_scaled = token_amount_to_ui_amount_v3(TEN_ATOM_D18_SCALE, &additional_data_scale);
        // 10.0 * 2.0 = 20.0
        assert!(token_amount_ten_scaled.ui_amount_string.starts_with("20"));
        assert!((token_amount_ten_scaled.ui_amount.unwrap() - 20.0).abs() < f64::EPSILON);

        // 场景2: 缩放因子为无穷大
        let config_inf_scale = ScaledUiAmountConfig {
            new_multiplier: f64::INFINITY.into(),
            ..Default::default()
        };
        let additional_data_inf_scale = SplTokenAdditionalDataV2 {
            decimals: 0,
            scaled_ui_amount_config: Some((config_inf_scale, 0)),
            ..Default::default()
        };
        let token_amount_inf_scale = token_amount_to_ui_amount_v3(u64::MAX, &additional_data_inf_scale);
        assert_eq!(token_amount_inf_scale.ui_amount, Some(f64::INFINITY));
        assert_eq!(token_amount_inf_scale.ui_amount_string, "inf");
    }

    // 测试函数 `test_ui_token_amount_real_string_zero`
    // 测试金额为0时，UI字符串的转换。
    #[test]
    fn test_ui_token_amount_real_string_zero() {
        // decimals = 0
        assert_eq!(&real_number_string(0, 0), "0");
        assert_eq!(&real_number_string_trimmed(0, 0), "0");
        let token_amount_0_0 =
            token_amount_to_ui_amount_v3(0, &SplTokenAdditionalDataV2::with_decimals(0));
        assert_eq!(token_amount_0_0.ui_amount_string, "0");
        assert_eq!(token_amount_0_0.ui_amount, Some(0.0));

        // decimals = 9
        assert_eq!(&real_number_string(0, 9), "0.000000000"); // 未trim时会显示所有小数位
        assert_eq!(&real_number_string_trimmed(0, 9), "0");   // trim后为 "0"
        let token_amount_0_9 =
            token_amount_to_ui_amount_v3(0, &SplTokenAdditionalDataV2::with_decimals(9));
        assert_eq!(token_amount_0_9.ui_amount_string, "0");
        assert_eq!(token_amount_0_9.ui_amount, Some(0.0));

        // decimals = 20 (较大)
        assert_eq!(&real_number_string(0, 25), "0.0000000000000000000000000");
        assert_eq!(&real_number_string_trimmed(0, 25), "0");
        let token_amount_0_20 =
            token_amount_to_ui_amount_v3(0, &SplTokenAdditionalDataV2::with_decimals(20));
        assert_eq!(token_amount_0_20.ui_amount_string, "0");
        // 对于0，即使decimals很大，f64值也应该是Some(0.0)，而不是None。
        // 之前的测试用例对非零值和大decimals时，ui_amount可能是None是正确的，因为精度问题。
        // 但对于0，0.0是精确的。
        assert_eq!(token_amount_0_20.ui_amount, Some(0.0));
    }

    // 测试函数 `test_parse_token_account_with_extensions`
    // 测试解析带有 Token-2022 扩展的 Token Account。
    #[test]
    fn test_parse_token_account_with_extensions() {
        let mint_pubkey = Pubkey::new_from_array([2; 32]);
        let owner_pubkey = Pubkey::new_from_array([3; 32]);

        // 基础账户数据
        let account_base = Account {
            mint: mint_pubkey,
            owner: owner_pubkey,
            amount: 42,
            state: AccountState::Initialized,
            is_native: COption::None,
            close_authority: COption::Some(owner_pubkey),
            delegate: COption::None,
            delegated_amount: 0,
        };

        // 计算包含 ImmutableOwner 和 MemoTransfer 扩展时账户所需的总长度
        let account_size = ExtensionType::try_calculate_account_len::<Account>(&[
            ExtensionType::ImmutableOwner,
            ExtensionType::MemoTransfer,
        ])
        .unwrap();
        let mut account_data_no_ext_init = vec![0; account_size]; // 分配空间
        // 使用 StateWithExtensionsMut 初始化账户数据，先不激活扩展
        let mut account_state_no_ext_init =
            StateWithExtensionsMut::<Account>::unpack_uninitialized(&mut account_data_no_ext_init).unwrap();
        account_state_no_ext_init.base = account_base; // 设置基础数据
        account_state_no_ext_init.pack_base();        // 将基础数据序列化到 account_data
        account_state_no_ext_init.init_account_type().unwrap(); // 标记账户类型为 Account

        // 断言1: 即使空间已为扩展分配，但如果扩展未被初始化，解析时不应显示它们。
        // 同时，解析 Token Account 仍需提供 decimals。
        assert!(parse_token_v3(&account_data_no_ext_init, None).is_err());
        assert_eq!(
            parse_token_v3(
                &account_data_no_ext_init,
                Some(&SplTokenAdditionalDataV2::with_decimals(2))
            )
            .unwrap(),
            TokenAccountType::Account(UiTokenAccount { // 结果与不带扩展的标准账户相同
                mint: mint_pubkey.to_string(),
                owner: owner_pubkey.to_string(),
                token_amount: UiTokenAmount {
                    ui_amount: Some(0.42), decimals: 2, amount: "42".to_string(), ui_amount_string: "0.42".to_string()
                },
                delegate: None, state: UiAccountState::Initialized, is_native: false,
                rent_exempt_reserve: None, delegated_amount: None,
                close_authority: Some(owner_pubkey.to_string()),
                extensions: vec![], // extensions 列表为空
            }),
        );

        // 现在初始化扩展
        let mut account_data_with_ext = vec![0; account_size];
        let mut account_state_with_ext =
            StateWithExtensionsMut::<Account>::unpack_uninitialized(&mut account_data_with_ext).unwrap();
        account_state_with_ext.base = account_base;
        account_state_with_ext.pack_base();
        account_state_with_ext.init_account_type().unwrap();

        // 初始化 ImmutableOwner 扩展
        account_state_with_ext
            .init_extension::<ImmutableOwner>(true) // true 表示如果扩展已存在则覆盖
            .unwrap();
        // 初始化 MemoTransfer 扩展并设置其属性
        let memo_transfer_ext = account_state_with_ext.init_extension::<MemoTransfer>(true).unwrap();
        memo_transfer_ext.require_incoming_transfer_memos = true.into(); // 设置需要转入备注

        // 断言2: 解析带有已初始化扩展的账户
        assert!(parse_token_v3(&account_data_with_ext, None).is_err()); // 仍然需要 decimals
        assert_eq!(
            parse_token_v3(
                &account_data_with_ext,
                Some(&SplTokenAdditionalDataV2::with_decimals(2))
            )
            .unwrap(),
            TokenAccountType::Account(UiTokenAccount {
                mint: mint_pubkey.to_string(), owner: owner_pubkey.to_string(),
                token_amount: UiTokenAmount { ui_amount: Some(0.42), decimals: 2, amount: "42".to_string(), ui_amount_string: "0.42".to_string() },
                delegate: None, state: UiAccountState::Initialized, is_native: false,
                rent_exempt_reserve: None, delegated_amount: None,
                close_authority: Some(owner_pubkey.to_string()),
                extensions: vec![ // extensions 列表现在应包含已解析的UI扩展
                    UiExtension::ImmutableOwner, // ImmutableOwner 扩展 (无额外数据)
                    UiExtension::MemoTransfer(UiMemoTransfer { // MemoTransfer 扩展及其数据
                        require_incoming_transfer_memos: true,
                    }),
                ],
            }),
        );
    }

    // 测试函数 `test_parse_token_mint_with_extensions`
    // 测试解析带有 Token-2022 扩展的 Mint Account。
    #[test]
    fn test_parse_token_mint_with_extensions() {
        let owner_pubkey = Pubkey::new_from_array([3; 32]); // 模拟的权限地址
        // 计算包含 MintCloseAuthority 扩展时 Mint 账户所需的总长度
        let mint_size_with_ext =
            ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::MintCloseAuthority])
                .unwrap();
        // 基础 Mint 数据
        let mint_base_data = Mint {
            mint_authority: COption::Some(owner_pubkey), supply: 42, decimals: 3,
            is_initialized: true, freeze_authority: COption::Some(owner_pubkey),
        };

        // 场景1: 扩展空间已分配，但扩展未初始化
        let mut mint_data_no_ext_init = vec![0; mint_size_with_ext];
        let mut mint_state_no_ext_init =
            StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut mint_data_no_ext_init).unwrap();
        mint_state_no_ext_init.base = mint_base_data;
        mint_state_no_ext_init.pack_base();
        mint_state_no_ext_init.init_account_type().unwrap(); // 标记为 Mint 类型

        // 解析 (Mint 解析不需要 additional_data)
        assert_eq!(
            parse_token_v3(&mint_data_no_ext_init, None).unwrap(),
            TokenAccountType::Mint(UiMint { // 结果与不带扩展的标准 Mint 相同
                mint_authority: Some(owner_pubkey.to_string()), supply: "42".to_string(), decimals: 3,
                is_initialized: true, freeze_authority: Some(owner_pubkey.to_string()),
                extensions: vec![], // extensions 列表为空
            }),
        );

        // 场景2: 扩展已初始化
        let mut mint_data_with_ext = vec![0; mint_size_with_ext];
        let mut mint_state_with_ext =
            StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut mint_data_with_ext).unwrap();

        // 初始化 MintCloseAuthority 扩展并设置其 close_authority
        let mint_close_auth_ext = mint_state_with_ext
            .init_extension::<MintCloseAuthority>(true) // true 表示如果扩展已存在则覆盖
            .unwrap();
        mint_close_auth_ext.close_authority =
            OptionalNonZeroPubkey::try_from(Some(owner_pubkey)).unwrap(); // 设置关闭权限

        // 设置基础数据并打包
        mint_state_with_ext.base = mint_base_data;
        mint_state_with_ext.pack_base();
        mint_state_with_ext.init_account_type().unwrap();

        // 解析带有已初始化扩展的 Mint 账户
        assert_eq!(
            parse_token_v3(&mint_data_with_ext, None).unwrap(),
            TokenAccountType::Mint(UiMint {
                mint_authority: Some(owner_pubkey.to_string()), supply: "42".to_string(), decimals: 3,
                is_initialized: true, freeze_authority: Some(owner_pubkey.to_string()),
                extensions: vec![ // extensions 列表现在应包含 MintCloseAuthority
                    UiExtension::MintCloseAuthority(UiMintCloseAuthority {
                        close_authority: Some(owner_pubkey.to_string()), // 对应的 UI 扩展数据
                    })],
            }),
        );
    }
}
