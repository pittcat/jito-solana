// 文件功能说明：
// 本文件是 `account-decoder` 库的入口文件，主要负责 Solana 账户数据的解码和转换。
// 它定义了如何将链上的原始账户数据转换成用户友好的、可读的格式 (UiAccount)。
// 这个库支持多种数据编码方式，例如 Base58、Base64、Base64+Zstd 以及人类可读的 JSON 格式。
// 它还提供了对账户数据进行切片的功能，允许用户只获取账户数据的一部分。
// 这个库在 Solana 区块链浏览器、钱包应用以及其他需要展示账户信息的工具中非常有用。

// Rust 特有概念解释：
// `#![allow(clippy::arithmetic_side_effects)]`: 这是一个属性宏，用于告诉编译器忽略 Clippy（一个 Rust 代码静态分析工具）
//                                            关于算术运算可能产生副作用（如溢出）的警告。在这个库中，可能有一些算术运算
//                                            被认为是安全的，或者溢出情况是可以接受的。
// `#[macro_use]`: 这个属性宏用于导入外部 crate（库）中的宏。
// `extern crate lazy_static;`: 导入 `lazy_static` crate。`lazy_static` 允许我们定义在运行时初始化一次并在后续使用中保持不变的静态变量。
//                             这对于定义一些全局常量或者需要复杂初始化的静态数据非常有用。
// `extern crate serde_derive;`: 导入 `serde_derive` crate。`serde` 是一个非常流行的 Rust 序列化/反序列化框架。
//                               `serde_derive` 提供了 `#[derive(Serialize, Deserialize)]` 这样的宏，可以自动为结构体和枚举
//                               生成序列化和反序列化的代码，使得数据在不同格式（如 JSON, Bincode）之间转换变得容易。

#![allow(clippy::arithmetic_side_effects)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

// 模块声明：
// `pub mod parse_account_data;` 等声明了多个公共模块。在 Rust 中，`mod` 关键字用于定义一个模块。
// 模块是组织代码的一种方式，可以将相关的函数、结构体、枚举等放在一个命名空间下。
// `pub` 关键字表示这个模块是公共的，可以被其他模块或 crate 访问。
// 这些模块分别负责解析不同类型的账户数据，例如：
// - `parse_account_data`: 通用的账户数据解析逻辑。
// - `parse_address_lookup_table`: 解析地址查找表账户。
// - `parse_bpf_loader`: 解析 BPF 加载器账户（用于存储和执行智能合约）。
// - `parse_config`: 解析配置账户。
// - `parse_nonce`: 解析 Nonce 账户（用于处理交易序列）。
// - `parse_stake`: 解析 Stake 账户（用于质押 SOL）。
// - `parse_sysvar`: 解析 Sysvar 账户（系统变量，如时钟、租金等）。
// - `parse_token`: 解析 Token 账户（SPL Token 标准）。
// - `parse_token_extension`: 解析 Token 扩展账户。
// - `parse_vote`: 解析 Vote 账户（用于投票验证者）。
// - `validator_info`: 解析验证者信息。
pub mod parse_account_data;
pub mod parse_address_lookup_table;
pub mod parse_bpf_loader;
#[allow(deprecated)] // `parse_config` 模块被标记为不推荐使用。
pub mod parse_config;
pub mod parse_nonce;
pub mod parse_stake;
pub mod parse_sysvar;
pub mod parse_token;
pub mod parse_token_extension;
pub mod parse_vote;
pub mod validator_info;

// `pub use` 语句：
// `pub use solana_account_decoder_client_types::{ UiAccount, UiAccountData, UiAccountEncoding, UiDataSliceConfig };`
// 这条语句从 `solana_account_decoder_client_types` crate 中导入并重新导出（re-export）了几个类型。
// 这意味着使用 `account-decoder` 库的开发者可以直接通过 `account_decoder::UiAccount` 等方式访问这些类型，
// 而不需要显式地依赖 `solana_account_decoder_client_types`。
// - `UiAccount`: 用户界面友好的账户表示结构体。
// - `UiAccountData`: 账户数据的不同编码格式的枚举。
// - `UiAccountEncoding`: 支持的账户数据编码类型枚举。
// - `UiDataSliceConfig`: 用于配置账户数据切片的结构体。
pub use solana_account_decoder_client_types::{
    UiAccount, UiAccountData, UiAccountEncoding, UiDataSliceConfig,
};

// `use` 语句：
// 导入当前 crate（库）或其他外部 crate 中的特定项（函数、结构体、枚举、模块等）。
use {
    crate::parse_account_data::{parse_account_data_v3, AccountAdditionalDataV3}, // 从当前 crate 的 `parse_account_data` 模块导入。
    base64::{prelude::BASE64_STANDARD, Engine}, // 从 `base64` crate 导入 Base64 编码相关的工具。
    solana_account::ReadableAccount,            // 从 `solana_account` crate 导入 `ReadableAccount` 特性（trait）。
                                                // `ReadableAccount` 定义了如何从一个账户对象中读取基本信息（如 lamports, data, owner 等）。
    solana_fee_calculator::FeeCalculator,       // 从 `solana_fee_calculator` crate 导入 `FeeCalculator` 结构体。
    solana_pubkey::Pubkey,                      // 从 `solana_pubkey` crate 导入 `Pubkey` 结构体，用于表示 Solana 地址。
    std::io::Write,                             // 从标准库的 `io` 模块导入 `Write` 特性，用于写入数据（例如在 Zstd 压缩时）。
};

// 类型别名：
// `pub type StringAmount = String;` 和 `pub type StringDecimals = String;`
// 定义了两个类型别名，将 `String` 类型分别命名为 `StringAmount` 和 `StringDecimals`。
// 这有助于提高代码的可读性，表明这些字符串特定用于表示金额和十进制数。
pub type StringAmount = String;
pub type StringDecimals = String;

// 常量定义：
// `pub const MAX_BASE58_BYTES: usize = 128;`
// 定义了一个公共常量 `MAX_BASE58_BYTES`，其值为 128。
// 这表示使用 Base58 编码时，原始数据的最大字节数限制。如果数据超过这个大小，Base58 编码可能会失败或不适用。
// Base58 是一种常用于区块链地址和密钥的编码方式，它不包含容易混淆的字符（如 0, O, I, l）。
pub const MAX_BASE58_BYTES: usize = 128;

// 函数：`encode_bs58`
// 功能：将账户数据编码为 Base58 字符串。
// 参数：
//   - `account`: 一个实现了 `ReadableAccount` 特性的账户对象。这意味着函数可以从 `account` 中获取数据。
//                `T: ReadableAccount` 是一个泛型参数，表示 `account` 可以是任何实现了 `ReadableAccount` 特性的类型。
//   - `data_slice_config`: 一个可选的 `UiDataSliceConfig`，用于指定只编码账户数据的一个切片。
//                         `Option` 是 Rust 中的一个枚举，表示一个值可能存在（`Some(value)`）或不存在（`None`）。
// 返回值：
//   - `String`: 编码后的 Base58 字符串。如果数据过大（超过 `MAX_BASE58_BYTES`），则返回错误信息字符串。
// 设计思路：
//   1. 首先使用 `slice_data` 函数根据 `data_slice_config` 获取账户数据的切片。
//   2. 检查切片长度是否超过 `MAX_BASE58_BYTES`。
//   3. 如果未超过，则使用 `bs58::encode` 进行编码并转换为字符串。
//   4. 如果超过，则返回一个固定的错误提示字符串。
fn encode_bs58<T: ReadableAccount>(
    account: &T, // `&T` 表示对 `T` 类型的不可变借用。借用是 Rust 所有权系统的一部分，允许在不转移所有权的情况下访问数据。
    data_slice_config: Option<UiDataSliceConfig>,
) -> String {
    // 调用 `slice_data` 函数获取可能被切片后的账户数据。
    let slice = slice_data(account.data(), data_slice_config);
    // 检查数据切片的长度是否小于或等于 `MAX_BASE58_BYTES`。
    if slice.len() <= MAX_BASE58_BYTES {
        // 如果长度合法，使用 `bs58` crate 的 `encode` 函数将数据编码为 Base58，然后转换为 `String`。
        bs58::encode(slice).into_string()
    } else {
        // 如果数据过长，返回错误信息。
        "error: data too large for bs58 encoding".to_string()
    }
}

// 函数：`encode_ui_account`
// 功能：将原始账户数据编码为一个 `UiAccount` 对象，该对象包含了用户友好的账户信息。
// 参数：
//   - `pubkey`: 账户的公钥（地址）。`&Pubkey` 表示对 `Pubkey` 的不可变借用。
//   - `account`: 实现了 `ReadableAccount` 特性的账户对象。
//   - `encoding`: `UiAccountEncoding` 枚举，指定账户数据的编码方式。
//   - `additional_data`: 可选的 `AccountAdditionalDataV3`，用于在解析 JSON 时提供额外上下文。
//   - `data_slice_config`: 可选的 `UiDataSliceConfig`，用于对账户数据进行切片。
// 返回值：
//   - `UiAccount`: 包含了编码后数据和其他账户信息的 `UiAccount` 结构体。
// 设计思路：
//   1. 获取账户数据的总长度（space）。
//   2. 使用 `match` 表达式根据指定的 `encoding` 类型来处理账户数据：
//      - `UiAccountEncoding::Binary` (已废弃的二进制格式，实际使用 Base58): 调用 `encode_bs58`，结果存入 `UiAccountData::LegacyBinary`。
//      - `UiAccountEncoding::Base58`: 调用 `encode_bs58`，结果存入 `UiAccountData::Binary`。
//      - `UiAccountEncoding::Base64`: 使用 `base64` crate 将（可能切片后的）数据编码为 Base64，结果存入 `UiAccountData::Binary`。
//      - `UiAccountEncoding::Base64Zstd`:
//        a. 尝试使用 Zstandard 算法压缩（可能切片后的）数据。
//        b. 如果压缩成功，将压缩后的数据进行 Base64 编码，结果存入 `UiAccountData::Binary`。
//        c. 如果压缩失败，则回退到仅使用 Base64 编码（不压缩），并将编码类型标记为 `UiAccountEncoding::Base64`。
//           `and_then` 是 `Result` 和 `Option` 常用的组合子，用于链式操作。
//      - `UiAccountEncoding::JsonParsed`:
//        a. 尝试调用 `parse_account_data_v3` 函数将账户数据解析为 JSON 格式。
//           `parse_account_data_v3` 会根据账户的 `owner` (程序ID) 来决定如何解析数据。
//        b. 如果解析成功，将 JSON 数据存入 `UiAccountData::Json`。
//        c. 如果解析失败（例如，账户类型未知或数据格式不正确），则回退到使用 Base64 编码数据，并将编码类型标记为 `UiAccountEncoding::Base64`。
//   3. 构建并返回 `UiAccount` 结构体，填充 lamports、编码后的数据、owner 地址、是否可执行、租金相关的 epoch 以及空间大小。
pub fn encode_ui_account<T: ReadableAccount>(
    pubkey: &Pubkey,
    account: &T,
    encoding: UiAccountEncoding,
    additional_data: Option<AccountAdditionalDataV3>,
    data_slice_config: Option<UiDataSliceConfig>,
) -> UiAccount {
    // 获取账户原始数据的长度。
    let space = account.data().len();
    // `match` 表达式是 Rust 中强大的控制流结构，类似于其他语言的 switch 语句，但更灵活。
    // 它会根据 `encoding` 的值选择相应的代码块执行。
    let data = match encoding {
        // 已废弃的二进制格式，现在使用 Base58 编码。
        UiAccountEncoding::Binary => {
            let data = encode_bs58(account, data_slice_config);
            UiAccountData::LegacyBinary(data) // LegacyBinary 表示旧的二进制格式，但内容是 Base58 编码的字符串。
        }
        // Base58 编码。
        UiAccountEncoding::Base58 => {
            let data = encode_bs58(account, data_slice_config);
            UiAccountData::Binary(data, encoding) // Binary 存储编码后的字符串和编码类型。
        }
        // Base64 编码。
        UiAccountEncoding::Base64 => UiAccountData::Binary(
            BASE64_STANDARD.encode(slice_data(account.data(), data_slice_config)), // 使用标准 Base64 引擎编码数据。
            encoding,
        ),
        // Base64 + Zstd 压缩编码。
        UiAccountEncoding::Base64Zstd => {
            // 创建一个 Zstd 编码器，输出到 Vec<u8> (动态数组)。级别 0 通常表示默认压缩级别。
            let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 0).unwrap();
            // `unwrap()` 用于处理 `Result` 类型，如果结果是 `Ok(value)`，则返回 `value`；如果是 `Err(e)`，则程序 panic。
            // 在这里，假设 Zstd 编码器总能成功创建。

            // 尝试写入数据并完成压缩。
            // `write_all` 将所有数据写入编码器。
            // `and_then` 用于链式处理 Result：如果前一个操作成功，则执行闭包内的操作。
            // `encoder.finish()` 完成压缩过程并返回包含压缩数据的 `Vec<u8>`。
            match encoder
                .write_all(slice_data(account.data(), data_slice_config))
                .and_then(|()| encoder.finish())
            {
                // `Ok(zstd_data)` 表示压缩成功。
                Ok(zstd_data) => UiAccountData::Binary(BASE64_STANDARD.encode(zstd_data), encoding),
                // `Err(_)` 表示压缩过程中发生错误 (用 `_` 忽略错误详情)。
                Err(_) => UiAccountData::Binary(
                    // 压缩失败，回退到只进行 Base64 编码。
                    BASE64_STANDARD.encode(slice_data(account.data(), data_slice_config)),
                    UiAccountEncoding::Base64, // 注意：编码类型也回退到 Base64。
                ),
            }
        }
        // JSON 解析格式。
        UiAccountEncoding::JsonParsed => {
            // 尝试将账户数据解析为特定的程序状态（JSON）。
            // `parse_account_data_v3` 是一个核心函数，它会根据账户的 `owner` (即哪个程序拥有这个账户)
            // 来调用相应的解析逻辑。例如，如果是 Token 账户，会用 Token 程序的解析器。
            if let Ok(parsed_data) =
                parse_account_data_v3(pubkey, account.owner(), account.data(), additional_data)
            {
                // `Ok(parsed_data)` 表示解析成功，`parsed_data` 是一个 `serde_json::Value` 类型。
                UiAccountData::Json(parsed_data)
            } else {
                // 解析失败（例如，账户类型不支持 JSON 解析，或者数据损坏），则回退到 Base64 编码。
                UiAccountData::Binary(
                    BASE64_STANDARD.encode(slice_data(account.data(), data_slice_config)),
                    UiAccountEncoding::Base64, // 编码类型也回退到 Base64。
                )
            }
        }
    };
    // 构建并返回 UiAccount 结构体。
    UiAccount {
        lamports: account.lamports(),                 // 账户余额 (单位：lamport，SOL 的最小单位)。
        data,                                         // 编码后的账户数据。
        owner: account.owner().to_string(),           // 账户所有者程序的地址字符串。
        executable: account.executable(),             //布尔值，指示账户是否包含可执行代码 (即是否为程序账户)。
        rent_epoch: account.rent_epoch(),             // 账户下次需要支付租金的 epoch (纪元)。Epoch 是 Solana 中的时间单位，代表一定数量的 slot (槽)。
        space: Some(space as u64),                    // 账户数据占用的空间大小 (字节)。 `Some(...)` 表示这是一个 `Option<u64>` 类型。
    }
}

// 结构体：`UiFeeCalculator`
// 功能：用于以用户友好的方式显示交易费用的计算器。
// 字段：
//   - `lamports_per_signature`: `StringAmount` (即 `String`) 类型，表示每次签名的费用（单位：lamport）。
// Rust 特有概念：
// `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]`:
//   - `Serialize`, `Deserialize`: 自动派生 `serde` 框架的序列化和反序列化能力，使得这个结构体可以方便地转换为 JSON 等格式，或从这些格式中创建。
//   - `Clone`: 允许通过 `.clone()` 方法创建这个结构体的深拷贝。
//   - `Debug`: 允许使用 `{:?}` 或 `{:#?}` 格式化操作符打印这个结构体以进行调试。
//   - `PartialEq`, `Eq`: 允许比较这个结构体的实例是否相等。`Eq` 要求所有字段都实现 `Eq`，`PartialEq` 则不要求。
// `#[serde(rename_all = "camelCase")]`: `serde` 的一个属性，指定在序列化和反序列化时，字段名使用驼峰命名法（camelCase）。
//                                     例如，`lamports_per_signature` 在 JSON 中会变成 `lamportsPerSignature`。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiFeeCalculator {
    pub lamports_per_signature: StringAmount,
}

// `impl From<FeeCalculator> for UiFeeCalculator`
// 功能：实现 `From` 特性，使得可以从 `solana_fee_calculator::FeeCalculator` 类型转换为 `UiFeeCalculator` 类型。
//      这提供了一种方便的类型转换方式，例如 `UiFeeCalculator::from(fee_calculator_instance)`。
// `From` 和 `Into` 是 Rust 中用于类型转换的一对核心特性。如果实现了 `From<T> for U`，那么会自动获得 `Into<U> for T`。
impl From<FeeCalculator> for UiFeeCalculator {
    fn from(fee_calculator: FeeCalculator) -> Self {
        Self {
            lamports_per_signature: fee_calculator.lamports_per_signature.to_string(), // 将 u64 类型的费用转换为字符串。
        }
    }
}

// `impl Default for UiFeeCalculator`
// 功能：实现 `Default` 特性，为 `UiFeeCalculator` 提供一个默认值。
//      可以通过 `UiFeeCalculator::default()` 获取一个默认实例。
impl Default for UiFeeCalculator {
    fn default() -> Self {
        Self {
            lamports_per_signature: "0".to_string(), // 默认情况下，每签名费用为 "0"。
        }
    }
}

// 函数：`slice_data`
// 功能：根据提供的 `UiDataSliceConfig`（偏移量和长度）从原始数据字节切片中提取子切片。
// 参数：
//   - `data`: `&[u8]`，原始数据的字节切片。`&[u8]` 是对一个 u8 数组（字节数组）的不可变借用。
//   - `data_slice_config`: `Option<UiDataSliceConfig>`，包含切片的偏移量（offset）和长度（length）。
// 返回值：
//   - `&[u8]`: 原始数据的子切片。如果配置无效（例如偏移量超出范围），则返回空切片。
// 设计思路：
//   1. 如果 `data_slice_config` 为 `Some`，则解构出 `offset` 和 `length`。
//   2. 检查 `offset` 是否超出 `data` 的长度：
//      - 如果是，说明请求的切片完全在数据范围之外，返回一个空切片 `&[]`。
//   3. 检查请求的 `length` 是否会超出从 `offset` 开始的数据末尾：
//      - 如果 `length` 延伸到数据末尾之外 (即 `length > data.len() - offset`)，则返回从 `offset` 到数据末尾的切片 `&data[offset..]`。
//      - 否则，返回从 `offset` 开始，长度为 `length` 的切片 `&data[offset..offset + length]`。
//   4. 如果 `data_slice_config` 为 `None`，则表示不需要切片，直接返回原始数据 `data`。
fn slice_data(data: &[u8], data_slice_config: Option<UiDataSliceConfig>) -> &[u8] {
    // `if let` 是 Rust 中一种方便的模式匹配方式，用于当只关心一种或少数几种模式时。
    // 这里匹配 `data_slice_config` 是否为 `Some(UiDataSliceConfig { offset, length })`。
    if let Some(UiDataSliceConfig { offset, length }) = data_slice_config {
        // 检查偏移量是否大于等于数据长度。
        if offset >= data.len() {
            // 如果偏移量超出范围，返回空切片。
            &[]
        } else if length > data.len() - offset {
            // 如果请求的长度超出了从偏移量开始的剩余数据长度，
            // 则返回从偏移量到数据末尾的切片。
            &data[offset..] // `offset..` 表示从 `offset` 到切片末尾。
        } else {
            // 否则，返回从 `offset` 开始，长度为 `length` 的切片。
            &data[offset..offset + length] // `offset..offset + length` 表示一个半开区间。
        }
    } else {
        // 如果没有提供切片配置，则返回整个数据。
        data
    }
}

// 测试模块：
// `#[cfg(test)]` 属性宏表示下面的 `mod test` 模块只在执行 `cargo test` 命令时编译。
// 这是 Rust 中编写单元测试和集成测试的标准方式。
#[cfg(test)]
mod test {
    // `use super::*;` 导入父模块（即本文件 `lib.rs`）中的所有公共项。
    // `assert_matches::assert_matches` 导入一个用于模式匹配断言的宏。
    // `solana_account::{Account, AccountSharedData}` 导入测试中需要用到的账户结构。
    use {
        super::*, // 导入父模块的所有公共项，使得可以直接使用 `encode_bs58`, `slice_data` 等。
        assert_matches::assert_matches, // 导入 `assert_matches` 宏，用于更方便地断言枚举成员和其内部值。
        solana_account::{Account, AccountSharedData}, // 导入 Solana 账户相关的结构体。
                                                    // `Account` 是一个基本的账户结构。
                                                    // `AccountSharedData` 是一个线程安全的账户数据容器，常用于在不同线程间共享账户状态。
    };

    // 测试函数：`test_slice_data`
    // 功能：测试 `slice_data` 函数的各种情况是否按预期工作。
    // `#[test]` 属性宏标记这是一个测试函数。
    #[test]
    fn test_slice_data() {
        // 创建一个包含 5 个字节的向量 (动态数组)。
        let data = vec![1, 2, 3, 4, 5];

        // 测试用例1：切片覆盖整个数据。
        let slice_config = Some(UiDataSliceConfig {
            offset: 0,
            length: 5,
        });
        // `assert_eq!` 宏断言两个表达式的值相等。
        assert_eq!(slice_data(&data, slice_config), &data[..]); // `&data[..]` 表示整个 `data` 切片。

        // 测试用例2：请求的长度超过数据实际长度，但偏移量为0。
        let slice_config = Some(UiDataSliceConfig {
            offset: 0,
            length: 10, // 长度大于 `data` 的长度。
        });
        // 预期结果：返回从偏移量开始到数据末尾的切片。
        assert_eq!(slice_data(&data, slice_config), &data[..]);

        // 测试用例3：正常的中间切片。
        let slice_config = Some(UiDataSliceConfig {
            offset: 1,
            length: 2,
        });
        // 预期结果：返回 `data` 中索引从 1 到 2 (不包括3) 的元素，即 `[2, 3]`。
        assert_eq!(slice_data(&data, slice_config), &data[1..3]);

        // 测试用例4：偏移量超出数据范围。
        let slice_config = Some(UiDataSliceConfig {
            offset: 10, // 偏移量远大于 `data` 的长度。
            length: 2,
        });
        // 预期结果：返回空切片。
        assert_eq!(slice_data(&data, slice_config), &[] as &[u8]); // `&[] as &[u8]` 创建一个空的 u8 切片。
    }

    // 测试函数：`test_encode_account_when_data_exceeds_base58_byte_limit`
    // 功能：测试当账户数据超过 `MAX_BASE58_BYTES` 限制时，`encode_bs58` 函数的行为。
    #[test]
    fn test_encode_account_when_data_exceeds_base58_byte_limit() {
        // 创建一个长度为 MAX_BASE58_BYTES + 2 的数据向量，所有元素都是 42。
        let data = vec![42; MAX_BASE58_BYTES + 2];
        // 创建一个包含上述数据的账户。
        // `AccountSharedData::from(Account { ... })` 将一个 `Account` 转换为 `AccountSharedData`。
        // `..Account::default()` 表示使用 `Account` 的默认值填充其余字段。
        let account = AccountSharedData::from(Account {
            data,
            ..Account::default()
        });

        // 测试用例1：对整个账户数据进行编码（不切片）。
        assert_eq!(
            encode_bs58(&account, None), // `None` 表示不进行切片。
            "error: data too large for bs58 encoding" // 预期返回错误信息。
        );

        // 测试用例2：切片后的数据仍然过大。
        assert_eq!(
            encode_bs58(
                &account,
                Some(UiDataSliceConfig {
                    length: MAX_BASE58_BYTES + 1, // 切片长度仍然超过限制。
                    offset: 1
                })
            ),
            "error: data too large for bs58 encoding" // 预期返回错误信息。
        );

        // 测试用例3：切片后的数据长度刚好在限制内。
        // `assert_ne!` 宏断言两个表达式的值不相等。
        assert_ne!(
            encode_bs58(
                &account,
                Some(UiDataSliceConfig {
                    length: MAX_BASE58_BYTES, // 切片长度等于限制。
                    offset: 1
                })
            ),
            "error: data too large for bs58 encoding" // 预期不会返回错误信息，即编码成功。
        );

        // 测试用例4：请求的切片长度过大，但实际从账户数据中切出的部分在限制内。
        // 例如，数据总长 130 (MAX_BASE58_BYTES=128)。offset=2, length=129。
        // 实际切出的数据是从索引 2 到末尾，长度为 130-2 = 128。
        assert_ne!(
            encode_bs58(
                &account,
                Some(UiDataSliceConfig {
                    length: MAX_BASE58_BYTES + 1, // 请求长度过大。
                    offset: 2                     // 但由于偏移量，实际切出的数据长度可能在限制内。
                })
            ),
            "error: data too large for bs58 encoding" // 预期不会返回错误信息。
        );
    }

    // 测试函数：`test_base64_zstd`
    // 功能：测试使用 `UiAccountEncoding::Base64Zstd` 编码和解码账户数据。
    #[test]
    fn test_base64_zstd() {
        // 创建一个包含 1024 个零字节数据的账户，并使用 Base64Zstd 编码。
        let encoded_account = encode_ui_account(
            &Pubkey::default(), // 使用默认的 Pubkey。
            &AccountSharedData::from(Account {
                data: vec![0; 1024], // 1KB 的数据。
                ..Account::default()
            }),
            UiAccountEncoding::Base64Zstd, // 指定编码方式。
            None,                          // 无附加数据。
            None,                          // 不切片。
        );

        // 使用 `assert_matches!` 宏断言 `encoded_account.data` 的模式。
        // 预期 `encoded_account.data` 是 `UiAccountData::Binary` 类型，
        // 且其内部的编码类型是 `UiAccountEncoding::Base64Zstd`。
        // `_` 表示我们不关心 Binary 内部的第一个字段（即编码后的字符串）的具体内容。
        assert_matches!(
            encoded_account.data,
            UiAccountData::Binary(_, UiAccountEncoding::Base64Zstd)
        );

        // 解码 `encoded_account` 回原始的 `Account` 类型。
        // `decode::<Account>()` 是 `UiAccount` 提供的一个泛型方法，用于解码数据。
        // `.unwrap()` 用于获取 `Result` 中的 `Ok` 值，如果解码失败则测试会 panic。
        let decoded_account = encoded_account.decode::<Account>().unwrap();
        // 断言解码后的数据与原始数据相同。
        assert_eq!(decoded_account.data(), &vec![0; 1024]);

        // 同样，解码为 `AccountSharedData` 类型并进行断言。
        let decoded_account = encoded_account.decode::<AccountSharedData>().unwrap();
        assert_eq!(decoded_account.data(), &vec![0; 1024]);
    }
}
