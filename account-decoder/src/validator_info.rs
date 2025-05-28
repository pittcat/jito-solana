// 文件功能说明：
// 这个文件定义了与 Solana 验证者信息 (Validator Info) 配置账户相关的常量和核心数据结构。
// 验证者信息账户是一种由 `solana-config-program` (配置程序) 管理的特殊账户，
// 它允许验证者在区块链上发布关于自己的公开信息。这些信息通常是一个 JSON 格式的字符串，
// 可以包含验证者的名称、网站链接、Logo图片地址、联系方式、简介等元数据。
//
// 这种机制的主要目的是：
// 1. 提高网络透明度：让社区成员和代币持有者更容易识别和了解 Solana 网络中的各个验证者。
// 2. 链上身份展示：为验证者提供一个在链上声明其品牌和身份的方式。
// 3. 工具集成便利：区块链浏览器、钱包应用、质押服务平台等工具可以读取这些标准化的信息，
//    向用户展示更丰富的验证者详情，从而帮助用户在选择质押对象时做出更明智的决策。
//
// 本文件主要包含：
// - 几个常量，用于限制验证者信息中某些字段（如果信息是结构化的）的最大长度，
//   以及整个验证者信息账户数据所占用的最大空间（以字节为单位）。
// - `ValidatorInfo` 结构体：这是验证者信息在链上存储的核心数据结构。它非常简单，
//   只有一个 `info` 字段，该字段是一个字符串，通常用于存储上述的 JSON 格式的验证者详细信息。
// - 为 `ValidatorInfo` 结构体实现了 `ConfigState` 特性。这个特性是与 `solana-config-program`
//   交互所必需的，它主要定义了该类型配置数据所允许的最大存储空间。
// - 通过 `solana_pubkey::declare_id!` 宏声明了一个固定的、人类可读的公钥（地址）：
//   `Va1idator1nfo111111111111111111111111111111`。这个公钥作为验证者信息配置账户
//   的一个“类型标识符”。当一个由配置程序管理的账户，其数据开头的 `ConfigKeys` 结构中
//   的第一个公钥是这个 ID 时，链上程序或链下工具就知道这个账户存储的是验证者信息。

// 从 `solana-config-program-client` crate 中导入 `ConfigState` 特性。
// `ConfigState` 特性通常由希望通过 `solana-config-program` 在链上存储其状态的结构体来实现。
// 它主要定义了该状态数据所需的最大存储空间。
use solana_config_program_client::instructions_bincode::ConfigState;

// 常量定义：
// 这些常量用于限制验证者信息中特定字段（如果信息被结构化解析）的长度。
// 虽然当前的 `ValidatorInfo` 结构只有一个 `info: String` 字段，这些常量可能预留给未来更结构化的信息，
// 或者用于链下工具在构造或验证 `info` 字符串内容时的参考。

// 短字段的最大长度，例如验证者名称、符号等。
pub const MAX_SHORT_FIELD_LENGTH: usize = 80; // 80 字节
// 长字段的最大长度，例如描述、URI等。
pub const MAX_LONG_FIELD_LENGTH: usize = 300; // 300 字节
// `ValidatorInfo` 结构序列化后（包括 `ConfigKeys` 等头部信息）所占用的最大总空间（以字节为单位）。
// 这个值会用于在创建验证者信息账户时分配合适的空间。
pub const MAX_VALIDATOR_INFO: u64 = 576;

// 声明一个固定的公钥ID，用于标识验证者信息类型的配置账户。
// `solana_pubkey::declare_id!` 是一个宏，它将一个 Base58 编码的字符串转换为编译时的 `Pubkey` 常量。
// 这个特定的ID "Va1idator1nfo111111111111111111111111111111" (注意大小写和数字1的使用，以满足Base58编码要求并易于识别)
// 在 `solana-config-program` 中被用作区分验证者信息配置账户的“键”或“类型ID”。
// 当解析一个通用的配置账户时，如果其 `ConfigKeys` 中的第一个公钥是这个ID，
// 那么就知道这个账户存储的是 `ValidatorInfo` 数据。
solana_pubkey::declare_id!("Va1idator1nfo111111111111111111111111111111");

// 结构体 `ValidatorInfo`：
// 这是存储在链上验证者信息配置账户中的核心数据结构。
// `#[derive(Debug, Deserialize, PartialEq, Eq, Serialize, Default)]`:
//   - `Debug`: 自动派生 `Debug` 特性，允许使用 `{:?}` 格式化操作符打印结构体实例，方便调试。
//   - `Deserialize`: 自动派生 `serde::Deserialize` 特性，使得这个结构体可以从序列化格式（如Bincode）中反序列化出来。
//   - `PartialEq`, `Eq`: 自动派生比较相等的特性，主要用于测试。
//   - `Serialize`: 自动派生 `serde::Serialize` 特性，使得这个结构体可以被序列化为其他格式（如Bincode, JSON）。
//   - `Default`: 自动派生 `Default` 特性，允许创建此结构体的默认实例（此时 `info` 字段会是一个空字符串）。
#[derive(Debug, Deserialize, PartialEq, Eq, Serialize, Default)]
pub struct ValidatorInfo {
    // `info`: 一个字符串字段，用于存储验证者的详细信息。
    // 通常，这个字符串的内容是一个 JSON 对象，包含键值对，例如：
    // `{"name": "My Validator", "website": "https://myvalidator.com", "keybase": "myusername"}`
    // 具体的 JSON 结构和字段由验证者社区或相关标准（如 Solana Foundation 的建议）约定。
    pub info: String,
}

// 为 `ValidatorInfo` 结构体实现 `ConfigState` 特性。
// Rust 特有概念：
//   - `impl Trait for Type`: 这是为类型 `Type` 实现特性 `Trait` 的标准语法。
//     特性（Trait）类似于其他语言中的接口（Interface），它定义了一组方法签名，
//     任何实现了该特性的类型都必须提供这些方法的具体实现。
impl ConfigState for ValidatorInfo {
    // `max_space()` 方法是 `ConfigState` 特性要求实现的方法。
    // 它返回这个配置状态（即 `ValidatorInfo` 结构体）序列化后所允许占用的最大空间（以字节为单位）。
    // 这个值通常用于在链上创建账户时，确保为该配置数据分配足够的存储空间。
    // Rust 特有概念：
    //   - `fn max_space() -> u64`: 定义一个关联函数（associated function，因为没有 `&self` 参数，
    //     所以它与类型本身相关联，而不是与类型的某个实例相关联）。
    //     它不需要参数，并返回一个 `u64` 类型的值。
    fn max_space() -> u64 {
        // 返回之前定义的常量 `MAX_VALIDATOR_INFO`。
        MAX_VALIDATOR_INFO
    }
}
