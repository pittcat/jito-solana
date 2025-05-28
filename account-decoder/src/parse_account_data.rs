// 文件功能说明：
// 本文件定义了将 Solana 链上各种类型的账户数据解析为人类可读的 JSON 格式的核心逻辑。
// Solana 账户的数据是二进制存储的，直接阅读非常困难。此文件通过识别账户的“所有者程序”（program_id），
// 然后调用该程序对应的特定解析函数，将二进制数据转换成结构化的 JSON 对象。
// 例如，如果一个账户属于 SPL Token Program，那么就会使用 `parse_token_v3` 函数来解析它。
// 这个模块是 Solana 区块链浏览器、钱包和其他开发者工具显示账户详细信息的基础。

// 重新导出 `ParsedAccount` 结构体，使其可以直接通过 `crate::parse_account_data::ParsedAccount` 访问。
// `ParsedAccount` 来自 `solana_account_decoder_client_types` crate，用于存储解析后的账户信息。
pub use solana_account_decoder_client_types::ParsedAccount;
use {
    crate::{
        // 导入各个特定类型账户的解析函数。
        // 每个函数都负责解析一种特定程序（如地址查找表、BPF加载器、配置、Nonce等）拥有的账户数据。
        parse_address_lookup_table::parse_address_lookup_table, // 解析地址查找表账户。
        parse_bpf_loader::parse_bpf_upgradeable_loader,         // 解析可升级的BPF加载器账户（通常用于存储智能合约）。
        parse_config::parse_config,                             // 解析配置账户。
        parse_nonce::parse_nonce,                               // 解析Nonce账户（用于交易序列化和防重放）。
        parse_stake::parse_stake,                               // 解析质押账户（用于网络安全和获取奖励）。
        parse_sysvar::parse_sysvar,                             // 解析系统变量账户（如时钟、租金信息等）。
        parse_token::parse_token_v3,                            // 解析SPL Token和SPL Token 2022标准的代币账户。
        parse_vote::parse_vote,                                 // 解析投票账户（验证者用于参与共识）。
    },
    inflector::Inflector, // 导入 `Inflector` 用于字符串格式转换（例如，将帕斯卡命名转换为烤串命名）。
    solana_clock::UnixTimestamp, // 导入 `UnixTimestamp` 类型，用于表示Unix时间戳。
    solana_instruction::error::InstructionError, // 导入指令执行错误类型。
    solana_pubkey::Pubkey,      // 导入 `Pubkey` 类型，用于表示Solana账户地址。
    solana_sdk_ids::{           // 导入各种内置程序的ID。这些ID是固定的公钥，用于识别特定的链上程序。
        address_lookup_table, bpf_loader_upgradeable, config, stake, system_program, sysvar, vote,
    },
    spl_token_2022::extension::{ // 从 SPL Token 2022 标准库中导入特定的扩展配置。
        interest_bearing_mint::InterestBearingConfig, // 生息代币的配置。
        scaled_ui_amount::ScaledUiAmountConfig,       // 用于UI显示的缩放金额配置。
    },
    std::collections::HashMap, // 导入标准库中的 `HashMap`，用于创建键值对映射。
    thiserror::Error,          // 导入 `thiserror` 宏，方便创建自定义错误类型。
};

// `lazy_static!` 宏：
// 这个宏允许我们定义在程序运行时才进行初始化的静态变量。
// 普通的 `static` 变量必须在编译时就知道其确切值，但 `lazy_static!` 定义的变量
// 可以在第一次被访问时执行初始化代码。这对于需要复杂计算或从运行时环境获取值的静态变量非常有用。
// 在这里，它被用来初始化几个程序ID的静态引用，以及一个包含所有可解析程序ID到对应解析器类型的映射。
lazy_static! {
    // 定义各种 Solana 内置程序的静态公钥ID。
    // `address_lookup_table::id()` 等函数返回对应程序的公钥。
    // `*` 操作符用于解引用，因为 `lazy_static!` 创建的是 `lazy_static::Lazy<T>` 类型，需要解引用才能得到 `T`。
    static ref ADDRESS_LOOKUP_PROGRAM_ID: Pubkey = address_lookup_table::id();
    static ref BPF_UPGRADEABLE_LOADER_PROGRAM_ID: Pubkey = bpf_loader_upgradeable::id();
    static ref CONFIG_PROGRAM_ID: Pubkey = config::id();
    static ref STAKE_PROGRAM_ID: Pubkey = stake::id();
    static ref SYSTEM_PROGRAM_ID: Pubkey = system_program::id(); // 系统程序，Nonce账户属于它。
    static ref SYSVAR_PROGRAM_ID: Pubkey = sysvar::id();
    static ref VOTE_PROGRAM_ID: Pubkey = vote::id();

    // `PARSABLE_PROGRAM_IDS`: 一个静态的 HashMap。
    // Key: `Pubkey` (程序ID)
    // Value: `ParsableAccount` (一个枚举，表示可解析的账户类型)
    // 这个 HashMap 用于快速查找一个给定的程序ID是否支持解析，以及它对应哪种账户类型。
    // 这是账户解析分发逻辑的核心数据结构。
    pub static ref PARSABLE_PROGRAM_IDS: HashMap<Pubkey, ParsableAccount> = {
        let mut m = HashMap::new(); // 创建一个新的 HashMap。
        // 将各个程序ID和对应的 `ParsableAccount` 枚举成员插入到 HashMap 中。
        m.insert(
            *ADDRESS_LOOKUP_PROGRAM_ID, // 地址查找表程序ID
            ParsableAccount::AddressLookupTable, // 对应的解析类型
        );
        m.insert(
            *BPF_UPGRADEABLE_LOADER_PROGRAM_ID, // 可升级BPF加载器程序ID
            ParsableAccount::BpfUpgradeableLoader,
        );
        m.insert(*CONFIG_PROGRAM_ID, ParsableAccount::Config); // 配置程序ID
        // Nonce 账户属于 System Program，所以这里用 SYSTEM_PROGRAM_ID。
        m.insert(*SYSTEM_PROGRAM_ID, ParsableAccount::Nonce);
        // SPL Token 和 SPL Token 2022 是两个不同版本的代币标准，它们有不同的程序ID。
        m.insert(spl_token::id(), ParsableAccount::SplToken); // SPL Token 程序ID
        m.insert(spl_token_2022::id(), ParsableAccount::SplToken2022); // SPL Token 2022 程序ID
        m.insert(*STAKE_PROGRAM_ID, ParsableAccount::Stake);   // 质押程序ID
        m.insert(*SYSVAR_PROGRAM_ID, ParsableAccount::Sysvar); // 系统变量程序ID
        m.insert(*VOTE_PROGRAM_ID, ParsableAccount::Vote);     // 投票程序ID
        m // 返回初始化完成的 HashMap。
    };
}

// 枚举 `ParseAccountError`:
// 定义了在解析账户数据过程中可能发生的各种错误。
// `#[derive(Error, Debug)]`:
//   - `Error`: 使用 `thiserror` crate 的宏，自动为枚举实现 `std::error::Error` 特性。
//              这使得此枚举可以作为标准的错误类型使用，例如在 `Result` 中返回。
//   - `Debug`: 自动派生 `Debug` 特性，允许使用 `{:?}` 格式化打印错误信息，方便调试。
// `#[error(...)]`: `thiserror` 提供的属性宏，用于为每个枚举成员定义用户友好的错误信息字符串。
#[derive(Error, Debug)]
pub enum ParseAccountError {
    // 当账户属于某个已知可解析的程序，但该特定账户类型无法解析时（例如，BPF Loader账户有多种状态）。
    // `{0:?}` 会被替换为 `ParsableAccount` 枚举成员的调试输出。
    #[error("{0:?} account not parsable")]
    AccountNotParsable(ParsableAccount),

    // 当账户的 `program_id` 不在 `PARSABLE_PROGRAM_IDS` 列表中时，表示该程序不被此解码器支持。
    #[error("Program not parsable")]
    ProgramNotParsable,

    // 当解析特定类型的账户（如 SPL Token）需要额外的上下文信息（如代币的精度）但未提供时。
    // `{0}` 会被替换为缺失的具体数据描述字符串。
    #[error("Additional data required to parse: {0}")]
    AdditionalDataMissing(String),

    // 当底层的指令反序列化或处理失败时。
    // `#[from] InstructionError` 表示这个错误成员可以从 `InstructionError` 类型自动转换而来。
    #[error("Instruction error")]
    InstructionError(#[from] InstructionError),

    // 当将解析结果序列化为 JSON 字符串时发生错误。
    // `#[from] serde_json::error::Error` 表示可以从 `serde_json` 的错误类型转换。
    #[error("Serde json error")]
    SerdeJsonError(#[from] serde_json::error::Error),
}

// 枚举 `ParsableAccount`:
// 列出了所有此解码器能够解析的账户类型。
// `#[derive(Debug, Serialize, Deserialize)]`:
//   - `Debug`: 允许调试打印。
//   - `Serialize`, `Deserialize`: 自动派生 `serde` 的序列化和反序列化能力。
//     这主要用于将此枚举作为 `ParsedAccount` 结构体的一部分进行序列化。
// `#[serde(rename_all = "camelCase")]`:
//   - 指定在序列化时，枚举成员的名称会转换为驼峰命名法（camelCase）。
//     例如 `AddressLookupTable` 会变成 `addressLookupTable`。
//     但在 `parse_account_data_v3` 函数中，通过 `format!("{:?}")` 和 `to_kebab_case()`，
//     最终输出到 `ParsedAccount.program` 字段的是烤串式命名（kebab-case），例如 `address-lookup-table`。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParsableAccount {
    AddressLookupTable,     // 地址查找表账户
    BpfUpgradeableLoader, // 可升级BPF加载器账户
    Config,                 // 配置账户
    Nonce,                  // Nonce账户
    SplToken,               // SPL Token 代币账户
    SplToken2022,           // SPL Token 2022 代币账户
    Stake,                  // 质押账户
    Sysvar,                 // 系统变量账户
    Vote,                   // 投票账户
}

// 结构体 `AccountAdditionalData`: (已废弃)
// 用于为账户解析提供额外数据，主要针对 SPL Token 账户，用于指定代币的精度（decimals）。
// `#[deprecated(...)]`: 标记此结构体已不推荐使用，并提示应使用 `AccountAdditionalDataV3`。
// `#[derive(Clone, Copy, Default)]`:
//   - `Clone`, `Copy`: 允许按值复制这个结构体。对于这种小且简单的结构体，`Copy` 是高效的。
//   - `Default`: 允许创建此结构体的默认实例，其中 `spl_token_decimals` 为 `None`。
#[deprecated(since = "2.0.0", note = "Use `AccountAdditionalDataV3` instead")]
#[derive(Clone, Copy, Default)]
pub struct AccountAdditionalData {
    pub spl_token_decimals: Option<u8>, // 可选的 SPL Token 精度。`Option<u8>` 表示它可能存在也可能不存在。
}

// 结构体 `AccountAdditionalDataV2`: (已废弃)
// `AccountAdditionalData` 的一个更新版本，引入了更结构化的 `SplTokenAdditionalData`。
// 同样被标记为已废弃，推荐使用 `AccountAdditionalDataV3`。
#[deprecated(since = "2.2.0", note = "Use `AccountAdditionalDataV3` instead")]
#[derive(Clone, Copy, Default)]
pub struct AccountAdditionalDataV2 {
    pub spl_token_additional_data: Option<SplTokenAdditionalData>, // 可选的 SPL Token 附加数据。
}

// 结构体 `AccountAdditionalDataV3`:
// 当前推荐使用的，为账户解析提供额外数据的结构体。
// 主要用于 SPL Token 账户，可以携带代币精度、生息配置和UI显示缩放配置。
#[derive(Clone, Copy, Default)]
pub struct AccountAdditionalDataV3 {
    pub spl_token_additional_data: Option<SplTokenAdditionalDataV2>, // 可选的 SPL Token 附加数据 (V2 版本，包含更多信息)。
}

// 实现 `From<AccountAdditionalDataV2> for AccountAdditionalDataV3`:
// 允许从旧版本的 `AccountAdditionalDataV2` 转换为 `AccountAdditionalDataV3`。
// 这是为了保持向后兼容性。
// `#[allow(deprecated)]` 允许在这个实现中使用已废弃的类型。
#[allow(deprecated)]
impl From<AccountAdditionalDataV2> for AccountAdditionalDataV3 {
    fn from(v: AccountAdditionalDataV2) -> Self {
        Self {
            // `map(Into::into)`: 如果 `v.spl_token_additional_data` 是 `Some(data)`，
            // 则调用 `data.into()` 将 `SplTokenAdditionalData` 转换为 `SplTokenAdditionalDataV2`。
            // 这是因为 `SplTokenAdditionalDataV2` 实现了 `From<SplTokenAdditionalData>`。
            spl_token_additional_data: v.spl_token_additional_data.map(Into::into),
        }
    }
}

// 结构体 `SplTokenAdditionalData`: (主要用于V2版本之前的附加数据)
// 存储 SPL Token 的附加信息，包括精度和可选的生息配置。
// `UnixTimestamp` 用于记录生息配置的生效时间。
#[derive(Clone, Copy, Default)]
pub struct SplTokenAdditionalData {
    pub decimals: u8, // 代币的精度 (小数点后的位数)。
    pub interest_bearing_config: Option<(InterestBearingConfig, UnixTimestamp)>, // 可选的生息配置和对应的时间戳。
}

impl SplTokenAdditionalData {
    // 构造函数，方便创建一个只包含 `decimals` 的实例。
    pub fn with_decimals(decimals: u8) -> Self {
        Self {
            decimals,
            ..Default::default() // 其余字段使用默认值 (`interest_bearing_config` 为 `None`)。
                                 // `..Default::default()` 是 Rust 中结构体更新语法的一种用法。
        }
    }
}

// 结构体 `SplTokenAdditionalDataV2`:
// 存储 SPL Token 的最新附加信息，在 `SplTokenAdditionalData` 的基础上增加了 `scaled_ui_amount_config`。
#[derive(Clone, Copy, Default)]
pub struct SplTokenAdditionalDataV2 {
    pub decimals: u8, // 代币精度。
    pub interest_bearing_config: Option<(InterestBearingConfig, UnixTimestamp)>, // 可选的生息配置。
    pub scaled_ui_amount_config: Option<(ScaledUiAmountConfig, UnixTimestamp)>, // 可选的UI缩放金额配置。
}

// 实现 `From<SplTokenAdditionalData> for SplTokenAdditionalDataV2`:
// 允许从旧的 `SplTokenAdditionalData` 转换为 `SplTokenAdditionalDataV2`。
// 新增的 `scaled_ui_amount_config` 字段会被设置为 `None`。
impl From<SplTokenAdditionalData> for SplTokenAdditionalDataV2 {
    fn from(v: SplTokenAdditionalData) -> Self {
        Self {
            decimals: v.decimals,
            interest_bearing_config: v.interest_bearing_config,
            scaled_ui_amount_config: None, // 新字段在转换时默认为 None。
        }
    }
}

impl SplTokenAdditionalDataV2 {
    // 构造函数，方便创建一个只包含 `decimals` 的实例。
    pub fn with_decimals(decimals: u8) -> Self {
        Self {
            decimals,
            ..Default::default() // 其余字段使用默认值。
        }
    }
}

// 函数 `parse_account_data`: (已废弃)
// 旧版本的账户数据解析入口函数。
// 它在内部调用了 `parse_account_data_v3`，并进行了必要的类型转换以保持兼容性。
// 参数：
//   - `pubkey`: `&Pubkey`，被解析账户的公钥。
//   - `program_id`: `&Pubkey`，拥有该账户的程序的公钥。
//   - `data`: `&[u8]`，账户的原始二进制数据。
//   - `additional_data`: `Option<AccountAdditionalData>`，可选的旧版附加数据。
// 返回值：
//   - `Result<ParsedAccount, ParseAccountError>`: 解析成功则返回 `Ok(ParsedAccount)`，失败则返回 `Err(ParseAccountError)`。
//     `Result` 是 Rust 中标准的错误处理枚举，`Ok` 表示成功并携带值，`Err` 表示失败并携带错误信息。
#[deprecated(since = "2.0.0", note = "Use `parse_account_data_v3` instead")]
#[allow(deprecated)] // 允许函数体内部使用已废弃的类型。
pub fn parse_account_data(
    pubkey: &Pubkey,
    program_id: &Pubkey,
    data: &[u8],
    additional_data: Option<AccountAdditionalData>,
) -> Result<ParsedAccount, ParseAccountError> {
    // 将旧的 `AccountAdditionalData` 转换为新的 `AccountAdditionalDataV3`。
    // `.map(|d| ...)`: 如果 `additional_data` 是 `Some(d)`，则执行闭包内的转换逻辑。
    // `d.spl_token_decimals.map(SplTokenAdditionalDataV2::with_decimals)`:
    //   如果 `d.spl_token_decimals` 是 `Some(decimals)`，则调用 `SplTokenAdditionalDataV2::with_decimals(decimals)`
    //   创建一个 `SplTokenAdditionalDataV2` 实例，否则结果为 `None`。
    //   然后这个 `Option<SplTokenAdditionalDataV2>` 被用来构造 `AccountAdditionalDataV3`。
    parse_account_data_v3(
        pubkey,
        program_id,
        data,
        additional_data.map(|d| AccountAdditionalDataV3 {
            spl_token_additional_data: d
                .spl_token_decimals
                .map(SplTokenAdditionalDataV2::with_decimals),
        }),
    )
}

// 函数 `parse_account_data_v2`: (已废弃)
// `parse_account_data` 的 V2 版本，同样已废弃。
// 内部调用 `parse_account_data_v3` 并转换 `AccountAdditionalDataV2`。
#[deprecated(since = "2.2.0", note = "Use `parse_account_data_v3` instead")]
#[allow(deprecated)]
pub fn parse_account_data_v2(
    pubkey: &Pubkey,
    program_id: &Pubkey,
    data: &[u8],
    additional_data: Option<AccountAdditionalDataV2>,
) -> Result<ParsedAccount, ParseAccountError> {
    // `additional_data.map(Into::into)`: 如果 `additional_data` 是 `Some(v2_data)`，
    // 则调用 `v2_data.into()` 将 `AccountAdditionalDataV2` 转换为 `AccountAdditionalDataV3`。
    // 这是因为 `AccountAdditionalDataV3` 实现了 `From<AccountAdditionalDataV2>`。
    parse_account_data_v3(pubkey, program_id, data, additional_data.map(Into::into))
}

// 函数 `parse_account_data_v3`:
// 当前主要的账户数据解析入口函数。
// 功能：根据账户的 `program_id` (所有者程序) 分发到相应的解析函数，并将原始账户数据转换为 `ParsedAccount` 结构。
// 参数：
//   - `pubkey`: `&Pubkey`，被解析账户的公钥。
//   - `program_id`: `&Pubkey`，账户的所有者程序ID。
//   - `data`: `&[u8]`，账户的原始二进制数据。
//   - `additional_data`: `Option<AccountAdditionalDataV3>`，可选的附加数据，目前主要用于SPL Token。
// 返回值：
//   - `Result<ParsedAccount, ParseAccountError>`: 包含解析结果或错误的 `Result`。
// 设计思路：
//   1. 使用 `PARSABLE_PROGRAM_IDS` (一个预定义的 HashMap) 查找 `program_id` 对应的 `ParsableAccount` 枚举成员。
//      如果找不到，说明该程序不被支持解析，返回 `ParseAccountError::ProgramNotParsable`。
//      `ok_or(...)` 是 `Option` 的一个方法，如果 `Option` 是 `Some(value)`，返回 `Ok(value)`；如果是 `None`，返回 `Err` 及提供的错误值。
//   2. 如果 `additional_data` 是 `None`，则使用默认值 `AccountAdditionalDataV3::default()`。
//      `unwrap_or_default()` 是 `Option` 的方法，如果是 `Some(value)` 返回 `value`，否则返回类型的默认值。
//   3. 使用 `match` 表达式根据 `program_name` (即 `ParsableAccount` 枚举成员) 选择对应的解析逻辑：
//      - 对每种 `ParsableAccount` 类型，调用其专属的解析函数（例如 `parse_address_lookup_table`）。
//      - 解析函数通常返回一个特定类型的结构体 (例如 `UiAddressLookupTable`)，这个结构体需要能被序列化为 JSON。
//      - `serde_json::to_value(...)` 将解析得到的结构体转换为 `serde_json::Value` (一种通用的 JSON 值表示)。
//      - `?` 操作符：用于错误传播。如果一个函数调用返回 `Result<T, E>`，在后面加上 `?`：
//        - 如果结果是 `Ok(value)`，则 `?` 表达式的值就是 `value`。
//        - 如果结果是 `Err(error)`，则当前函数会立即返回 `Err(error)` (要求当前函数的返回类型也是 `Result<_, E>`)。
//   4. 如果所有步骤成功，则构建一个 `ParsedAccount` 结构体：
//      - `program`: 将 `ParsableAccount` 枚举成员的名称转换为烤串式命名 (e.g., "address-lookup-table")。
//                   `format!("{program_name:?}")` 将枚举成员转为其调试字符串形式 (e.g., "AddressLookupTable")。
//                   `.to_kebab_case()` 是 `Inflector` crate 提供的方法，将其转换为烤串式。
//      - `parsed`: 存储 `serde_json::Value` 格式的解析结果。
//      - `space`: 存储账户原始数据的长度 (字节数)。
//   5. 返回 `Ok(ParsedAccount)`。
pub fn parse_account_data_v3(
    pubkey: &Pubkey,
    program_id: &Pubkey,
    data: &[u8],
    additional_data: Option<AccountAdditionalDataV3>,
) -> Result<ParsedAccount, ParseAccountError> {
    // 1. 查找 program_id 对应的 ParsableAccount 类型。
    let program_name = PARSABLE_PROGRAM_IDS
        .get(program_id) // 在 HashMap 中查找 program_id。返回 Option<&ParsableAccount>。
        .ok_or(ParseAccountError::ProgramNotParsable)?; // 如果找不到，返回 ProgramNotParsable 错误。

    // 2. 处理附加数据，如果为 None 则使用默认值。
    let additional_data = additional_data.unwrap_or_default();

    // 3. 根据 program_name 匹配并调用相应的解析函数。
    let parsed_json = match program_name {
        ParsableAccount::AddressLookupTable => {
            // 调用地址查找表解析函数，并将结果转为 serde_json::Value。
            serde_json::to_value(parse_address_lookup_table(data)?)?
        }
        ParsableAccount::BpfUpgradeableLoader => {
            // 调用 BPF 可升级加载器解析函数。
            serde_json::to_value(parse_bpf_upgradeable_loader(data)?)?
        }
        ParsableAccount::Config => serde_json::to_value(parse_config(data, pubkey)?)?, // 配置账户解析。
        ParsableAccount::Nonce => serde_json::to_value(parse_nonce(data)?)?,          // Nonce 账户解析。
        // SPL Token 和 SPL Token 2022 共用一个解析分支，但底层的 `parse_token_v3` 可能会根据数据内容区分它们。
        // 或者 `parse_token_v3` 只处理共性，特定版本的细节由其内部逻辑或依赖的库处理。
        // `additional_data.spl_token_additional_data.as_ref()` 将 `Option<T>` 转为 `Option<&T>`，
        // 因为 `parse_token_v3` 需要的是对附加数据的引用。
        ParsableAccount::SplToken | ParsableAccount::SplToken2022 => serde_json::to_value(
            parse_token_v3(data, additional_data.spl_token_additional_data.as_ref())?,
        )?,
        ParsableAccount::Stake => serde_json::to_value(parse_stake(data)?)?,          // 质押账户解析。
        ParsableAccount::Sysvar => serde_json::to_value(parse_sysvar(data, pubkey)?)?, // 系统变量账户解析。
        ParsableAccount::Vote => serde_json::to_value(parse_vote(data)?)?,            // 投票账户解析。
    };

    // 4. 构建并返回 ParsedAccount 结构体。
    Ok(ParsedAccount {
        program: format!("{program_name:?}").to_kebab_case(), // 将程序类型名转为烤串式。
        parsed: parsed_json,                                  // 解析后的 JSON 数据。
        space: data.len() as u64,                             // 账户数据空间大小。
    })
}

// 测试模块
#[cfg(test)]
mod test {
    // 导入父模块的所有项 (`super::*`) 和测试中需要的特定项。
    use {
        super::*, // 导入父模块（`crate::parse_account_data`）中的所有公共项。
        solana_nonce::{ // 导入 Nonce 相关的状态和版本控制结构。
            state::{Data, State as NonceState}, // Nonce 账户的具体数据和状态枚举。重命名 `State` 为 `NonceState` 以避免与 `VoteState` 冲突。
            versions::Versions as NonceVersions, // Nonce 账户状态的版本管理。
        },
        solana_vote_interface::{ // 导入投票程序接口相关的项。
            program::id as vote_program_id, // 获取投票程序的ID（公钥）。
            state::{VoteState, VoteStateVersions}, // 投票账户的状态和版本控制。
        },
    };

    // 测试函数 `test_parse_account_data`
    // `#[test]` 宏标记这是一个单元测试函数，只有在执行 `cargo test` 时才会编译和运行。
    #[test]
    fn test_parse_account_data() {
        // 创建一个随机的账户公钥，用于测试。
        let account_pubkey = solana_pubkey::new_rand();
        // 创建一个随机的程序公钥，模拟一个不被支持解析的程序。
        let other_program = solana_pubkey::new_rand();
        // 创建一些任意的二进制数据。
        let data = vec![0; 4];
        // 断言：尝试解析一个由未知程序拥有的账户时，应该返回错误。
        // `is_err()` 检查 `Result` 是否为 `Err` 变体。
        assert!(parse_account_data_v3(&account_pubkey, &other_program, &data, None).is_err());

        // 测试投票账户的解析：
        let vote_state = VoteState::default(); // 创建一个默认的投票状态。
        // 创建一个足够大的字节向量来存储序列化后的投票状态。
        // `VoteState::size_of()` 获取投票状态结构体的大小。
        let mut vote_account_data: Vec<u8> = vec![0; VoteState::size_of()];
        // 将投票状态包装在版本控制结构中（使用当前版本）。
        let versioned = VoteStateVersions::new_current(vote_state);
        // 将版本化的投票状态序列化到 `vote_account_data` 字节向量中。
        // `.unwrap()` 用于处理 `Result`，如果序列化失败则测试会 panic。
        VoteState::serialize(&versioned, &mut vote_account_data).unwrap();

        // 调用 `parse_account_data_v3` 解析投票账户数据。
        let parsed = parse_account_data_v3(
            &account_pubkey,      // 任意账户公钥
            &vote_program_id(),   // 投票程序的公钥
            &vote_account_data,   // 序列化后的投票数据
            None,                 // 无附加数据
        )
        .unwrap(); // 如果解析失败则 panic。

        // 断言：解析得到的程序名应该是 "vote" (烤串式命名)。
        assert_eq!(parsed.program, "vote".to_string());
        // 断言：解析得到的空间大小应该与 `VoteState::size_of()` 相等。
        assert_eq!(parsed.space, VoteState::size_of() as u64);

        // 测试 Nonce 账户的解析：
        // 创建一个初始化的 Nonce 状态数据。
        let nonce_data = NonceVersions::new(NonceState::Initialized(Data::default()));
        // 使用 `bincode` 将 Nonce 数据序列化为字节向量。
        // `bincode` 是一个流行的二进制序列化/反序列化库。
        let nonce_account_data = bincode::serialize(&nonce_data).unwrap();

        // 调用 `parse_account_data_v3` 解析 Nonce 账户数据。
        // Nonce 账户属于系统程序 (`system_program::id()`)。
        let parsed = parse_account_data_v3(
            &account_pubkey,
            &system_program::id(), // Nonce 账户由系统程序拥有。
            &nonce_account_data,
            None,
        )
        .unwrap();

        // 断言：解析得到的程序名应该是 "nonce"。
        assert_eq!(parsed.program, "nonce".to_string());
        // 断言：解析得到的空间大小应该与 `NonceState::size()` 相等。
        assert_eq!(parsed.space, NonceState::size() as u64);
    }
}
