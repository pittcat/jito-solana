// 文件功能说明：
// 这个文件专门用于解析 Solana 上的 BPF 可升级加载器 (BPF Upgradeable Loader) 程序所拥有的账户数据。
// BPF (Berkeley Packet Filter) 是 Solana 用来执行智能合约 (在 Solana 中称为“程序”) 的底层虚拟机技术。
// “可升级加载器”意味着部署在链上的程序可以在其授权地址的许可下进行升级，这为程序开发者提供了灵活性。
//
// BPF 可升级加载器账户可以有以下几种不同的状态，每种状态都有其特定的数据结构：
// 1. Uninitialized: 账户已创建，但尚未被赋予任何 BPF 加载器相关的状态。
// 2. Buffer:      当开发者想要部署或升级一个程序时，他们首先会将程序的字节码（编译后的代码）上传到一个 Buffer 类型的账户中。
//                 这个 Buffer 账户会有一个管理者 (authority)，负责后续的部署操作。
// 3. Program:     一旦 Buffer 中的代码被验证并部署，就会创建一个 Program 类型的账户。这个账户本身并不直接存储程序的字节码，
//                 而是存储一个指向实际存储字节码的 ProgramData 账户的地址。Program 账户是用户与之交互的“程序入口点”。
// 4. ProgramData: 这个类型的账户实际存储了已部署程序的字节码。它还包含了程序最后部署或升级的 slot（槽，可以理解为 Solana 上的时间戳或区块高度），
//                 以及一个可选的升级管理者 (upgrade authority)，该管理者有权修改这个 ProgramData 账户的内容，从而实现程序升级。
//
// 本文件中的代码会将这些不同状态账户的原始二进制数据，通过反序列化（deserialize）和类型匹配，
// 转换成易于人类阅读和程序处理的结构化信息（通常是 JSON 格式），方便钱包、浏览器等工具展示程序账户的详细状态。

use {
    // 从当前 crate (solana-account-decoder) 的 `parse_account_data` 模块中导入 `ParsableAccount` 和 `ParseAccountError`。
    // `ParsableAccount` 是一个枚举，列出了所有可以被解析的账户类型。
    // `ParseAccountError` 是一个枚举，定义了在解析过程中可能发生的错误。
    crate::{
        parse_account_data::{ParsableAccount, ParseAccountError},
        UiAccountData, UiAccountEncoding, // `UiAccountData` 和 `UiAccountEncoding` 用于表示账户数据（通常是 Base64 编码的）。
    },
    base64::{prelude::BASE64_STANDARD, Engine}, // 导入 Base64 编码/解码库，用于处理二进制数据。
    bincode::{deserialize, serialized_size},   // 导入 `bincode` 库，用于二进制序列化和反序列化。
                                                // `deserialize` 用于将字节数据转换为 Rust 结构体。
                                                // `serialized_size` 用于计算一个结构体序列化后的大小。
    solana_loader_v3_interface::state::UpgradeableLoaderState, // 从 Solana 加载器接口库导入 `UpgradeableLoaderState` 枚举。
                                                              // 这个枚举定义了 BPF 可升级加载器账户在链上的各种可能状态及其原始数据结构。
    solana_pubkey::Pubkey, // 导入 `Pubkey` 类型，用于表示 Solana 账户的公钥（地址）。
};

// 函数 `parse_bpf_upgradeable_loader`：
// 功能：解析 BPF 可升级加载器账户的原始二进制数据。
// 参数：
//   - `data`: `&[u8]`，一个字节切片，代表 BPF 可升级加载器账户的原始数据。
// 返回值：
//   - `Result<BpfUpgradeableLoaderAccountType, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(BpfUpgradeableLoaderAccountType)`，其中 `BpfUpgradeableLoaderAccountType` 是一个枚举，
//       包含了对应账户状态的详细信息（如 Uninitialized, Buffer, Program, ProgramData）。
//     - 如果解析失败（例如数据格式不正确），返回 `Err(ParseAccountError)`。
// 设计思路：
//   1. 首先，尝试使用 `bincode::deserialize(data)` 将传入的原始字节数据 `data` 反序列化为 `UpgradeableLoaderState` 枚举。
//      `UpgradeableLoaderState` 是链上存储的原始状态表示。
//      如果反序列化失败，说明数据不是一个有效的 BPF 可升级加载器账户状态，此时映射错误为
//      `ParseAccountError::AccountNotParsable(ParsableAccount::BpfUpgradeableLoader)` 并返回。
//      `.map_err(|_| ...)` 是 `Result` 的一个方法，用于在 `Err` 的情况下转换错误类型。`_` 忽略原始的 bincode 错误。
//   2. 如果反序列化成功，我们得到了 `account_state`，这是一个 `UpgradeableLoaderState` 的实例。
//   3. 接下来，使用 `match` 表达式根据 `account_state` 的不同成员（即账户的不同状态）进行处理：
//      - `UpgradeableLoaderState::Uninitialized`: 直接映射为 `BpfUpgradeableLoaderAccountType::Uninitialized`。
//      - `UpgradeableLoaderState::Buffer { authority_address }`:
//        - 这种状态下，账户数据除了元数据（如 `authority_address`）外，还包含了实际的程序字节码。
//        - 我们需要计算元数据部分的大小 (`offset`)，以便能正确提取出原始的程序字节码。
//        - `UpgradeableLoaderState::size_of_buffer_metadata()` 给出了包含 `authority_address` (如果存在) 的元数据大小。
//        - 如果 `authority_address` 为 `None` (理论上 Buffer 账户总会有 authority，但代码为了完整性处理此情况)，
//          则从元数据大小中减去一个 `Pubkey` 的序列化大小。
//        - 提取 `data[offset..]` 作为程序字节码，并将其进行 Base64 编码，存储在 `UiBuffer` 的 `data` 字段中。
//        - 将 `authority_address` (如果存在) 转换为字符串，存储在 `UiBuffer` 的 `authority` 字段中。
//        - 最后，将构造好的 `UiBuffer` 包装在 `BpfUpgradeableLoaderAccountType::Buffer` 中。
//      - `UpgradeableLoaderState::Program { programdata_address }`:
//        - 这种状态表示一个已部署的程序。它只包含一个指向 `ProgramData` 账户的地址 `programdata_address`。
//        - 将 `programdata_address` 转换为字符串，存储在 `UiProgram` 的 `program_data` 字段中。
//        - 将构造好的 `UiProgram` 包装在 `BpfUpgradeableLoaderAccountType::Program` 中。
//      - `UpgradeableLoaderState::ProgramData { slot, upgrade_authority_address }`:
//        - 这种状态表示存储实际程序字节码的账户。
//        - 类似于 `Buffer` 状态，需要计算元数据的大小 (`offset`) 来提取程序字节码。
//        - `UpgradeableLoaderState::size_of_programdata_metadata()` 给出了包含 `slot` 和 `upgrade_authority_address` (如果存在) 的元数据大小。
//        - 提取 `data[offset..]` 作为程序字节码，并进行 Base64 编码，存储在 `UiProgramData` 的 `data` 字段中。
//        - 将 `slot` 和 `upgrade_authority_address` (如果存在，转换为字符串) 存储在 `UiProgramData` 中。
//        - 将构造好的 `UiProgramData` 包装在 `BpfUpgradeableLoaderAccountType::ProgramData` 中。
//   4. 将匹配和转换后得到的 `parsed_account` (类型为 `BpfUpgradeableLoaderAccountType`) 包装在 `Ok()` 中并返回。
pub fn parse_bpf_upgradeable_loader(
    data: &[u8],
) -> Result<BpfUpgradeableLoaderAccountType, ParseAccountError> {
    // 1. 尝试将原始数据反序列化为链上状态 UpgradeableLoaderState
    let account_state: UpgradeableLoaderState = deserialize(data).map_err(|_| {
        // 如果反序列化失败，返回特定的解析错误
        ParseAccountError::AccountNotParsable(ParsableAccount::BpfUpgradeableLoader)
    })?;

    // 2. 根据反序列化得到的账户状态，匹配并转换为用户友好的 BpfUpgradeableLoaderAccountType
    let parsed_account = match account_state {
        // 状态：未初始化
        UpgradeableLoaderState::Uninitialized => BpfUpgradeableLoaderAccountType::Uninitialized,
        // 状态：缓冲区 (用于上传程序代码)
        UpgradeableLoaderState::Buffer { authority_address } => {
            // 计算元数据在字节数据中的偏移量，以提取实际的程序代码
            let offset = if authority_address.is_some() {
                // 如果存在 authority 地址，元数据大小是固定的
                UpgradeableLoaderState::size_of_buffer_metadata()
            } else {
                // 理论上 Buffer 账户总是有 authority_address。
                // 此处为代码完整性处理 authority_address 为 None 的情况：
                // 从元数据大小中减去一个 Pubkey 的序列化大小。
                UpgradeableLoaderState::size_of_buffer_metadata()
                    - serialized_size(&Pubkey::default()).unwrap() as usize
            };
            // 构建 UiBuffer 结构体
            BpfUpgradeableLoaderAccountType::Buffer(UiBuffer {
                authority: authority_address.map(|pubkey| pubkey.to_string()), // 将 Option<Pubkey> 转为 Option<String>
                data: UiAccountData::Binary( // 将原始程序字节码进行 Base64 编码
                    BASE64_STANDARD.encode(&data[offset..]), // data[offset..] 是实际的程序字节码切片
                    UiAccountEncoding::Base64,
                ),
            })
        }
        // 状态：已部署的程序
        UpgradeableLoaderState::Program {
            programdata_address, // 指向存储程序字节码的 ProgramData 账户的地址
        } => BpfUpgradeableLoaderAccountType::Program(UiProgram {
            program_data: programdata_address.to_string(), // 将 ProgramData 地址转为字符串
        }),
        // 状态：存储程序字节码的数据账户
        UpgradeableLoaderState::ProgramData {
            slot, // 程序最后部署或升级的 slot (槽/区块高度)
            upgrade_authority_address, // 可选的升级权限地址
        } => {
            // 计算元数据偏移量，以提取实际的程序代码
            let offset = if upgrade_authority_address.is_some() {
                UpgradeableLoaderState::size_of_programdata_metadata()
            } else {
                // 如果没有升级权限地址，元数据会小一些
                UpgradeableLoaderState::size_of_programdata_metadata()
                    - serialized_size(&Pubkey::default()).unwrap() as usize
            };
            // 构建 UiProgramData 结构体
            BpfUpgradeableLoaderAccountType::ProgramData(UiProgramData {
                slot, // u64 类型，直接使用
                authority: upgrade_authority_address.map(|pubkey| pubkey.to_string()), // Option<Pubkey> 转 Option<String>
                data: UiAccountData::Binary( // 将原始程序字节码进行 Base64 编码
                    BASE64_STANDARD.encode(&data[offset..]),
                    UiAccountEncoding::Base64,
                ),
            })
        }
    };
    // 3. 返回成功解析的结果
    Ok(parsed_account)
}

// 枚举 `BpfUpgradeableLoaderAccountType`:
// 表示 BPF 可升级加载器账户的几种不同类型（状态），并包装了对应状态的用户友好数据结构。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`:
//   - `Debug`: 允许使用 `{:?}` 格式化打印，方便调试。
//   - `Serialize`, `Deserialize`: 自动派生 `serde` 的序列化和反序列化能力，使其可以轻松转换为 JSON 等格式。
//   - `PartialEq`, `Eq`: 允许比较此枚举的实例是否相等，主要用于测试。
// `#[serde(rename_all = "camelCase", tag = "type", content = "info")]`:
//   - `rename_all = "camelCase"`: 在序列化为 JSON 时，枚举成员名和字段名将转换为驼峰式命名 (e.g., "programData")。
//   - `tag = "type"`: 在序列化时，JSON 对象会额外添加一个名为 "type" 的字段，其值为当前枚举成员的名称
//     (例如，对于 `ProgramData` 成员，其 JSON 中会有 `"type": "programData"`)。这有助于区分不同的账户类型。
//   - `content = "info"`: 对于包含数据的枚举成员（如 `Buffer(UiBuffer)`），其内部的数据（`UiBuffer`）
//     会序列化到一个名为 "info" 的 JSON 字段中。
// 例如，一个 `Buffer` 类型的账户会序列化成类似：`{ "type": "buffer", "info": { ... UiBuffer 的内容 ... } }`
// 而一个 `Uninitialized` 类型的账户会序列化成：`{ "type": "uninitialized" }`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum BpfUpgradeableLoaderAccountType {
    Uninitialized,            // 未初始化状态
    Buffer(UiBuffer),         // 缓冲区状态，包含 UiBuffer 数据
    Program(UiProgram),       // 程序状态，包含 UiProgram 数据
    ProgramData(UiProgramData), // 程序数据状态，包含 UiProgramData 数据
}

// 结构体 `UiBuffer`:
// 用于以用户友好的方式显示 `Buffer` 类型账户的信息。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`: 派生常用特性。
// `#[serde(rename_all = "camelCase")]`: 序列化为 JSON 时字段名使用驼峰式。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiBuffer {
    // `authority`: 可选的，缓冲区的管理者地址。
    // 如果为 `Some(String)`，表示该地址有权部署此缓冲区中的代码。
    // 如果为 `None`，理论上不常见于实际的 Buffer 账户。
    pub authority: Option<String>,
    // `data`: 存储在缓冲区中的程序字节码。
    // `UiAccountData` 通常是一个枚举，这里会用其 `Binary` 成员来存储 Base64 编码后的字节码字符串。
    pub data: UiAccountData,
}

// 结构体 `UiProgram`:
// 用于以用户友好的方式显示 `Program` 类型账户的信息。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`: 派生常用特性。
// `#[serde(rename_all = "camelCase")]`: 序列化为 JSON 时字段名使用驼峰式。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiProgram {
    // `program_data`: 存储已部署程序实际字节码的 `ProgramData` 账户的地址（公钥字符串）。
    pub program_data: String,
}

// 结构体 `UiProgramData`:
// 用于以用户友好的方式显示 `ProgramData` 类型账户的信息。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`: 派生常用特性。
// `#[serde(rename_all = "camelCase")]`: 序列化为 JSON 时字段名使用驼峰式。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiProgramData {
    // `slot`: 程序数据最后被写入或修改时的 slot (槽/区块高度)。
    // 这可以用来追踪程序的部署或升级历史。
    pub slot: u64,
    // `authority`: 可选的，有权升级此程序数据的管理者地址。
    // 如果为 `Some(String)`，表示该地址可以修改存储的程序字节码。
    // 如果为 `None`，则该程序字节码是不可变的（除非整个 ProgramData 账户被关闭）。
    pub authority: Option<String>,
    // `data`: 实际存储的程序字节码。
    // `UiAccountData` 通常用其 `Binary` 成员来存储 Base64 编码后的字节码字符串。
    pub data: UiAccountData,
}

// 测试模块
// `#[cfg(test)]` 属性宏表示下面的 `mod test` 模块只在执行 `cargo test` 命令时编译和运行。
#[cfg(test)]
mod test {
    // 导入父模块（即本文件 `parse_bpf_loader.rs`）中的所有公共项 (`super::*`)。
    // 同时导入 `bincode::serialize` 用于在测试中将状态序列化为字节，以及 `solana_pubkey::Pubkey`。
    use {super::*, bincode::serialize, solana_pubkey::Pubkey};

    // 测试函数 `test_parse_bpf_upgradeable_loader_accounts`
    // `#[test]` 宏标记这是一个单元测试函数。
    #[test]
    fn test_parse_bpf_upgradeable_loader_accounts() {
        // 1. 测试 Uninitialized 状态
        let bpf_loader_state = UpgradeableLoaderState::Uninitialized;
        // 将状态序列化为字节数据
        let account_data = serialize(&bpf_loader_state).unwrap();
        // 解析并断言结果
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Uninitialized
        );

        // 准备一些任意的程序字节码用于后续测试
        let program = vec![7u8; 64]; // 64 个值为 7 的字节

        // 2. 测试 Buffer 状态 (有 authority)
        let authority = Pubkey::new_unique(); // 创建一个随机的管理者地址
        let bpf_loader_state = UpgradeableLoaderState::Buffer {
            authority_address: Some(authority), // 设置 authority
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap(); // 序列化元数据部分
        account_data.extend_from_slice(&program); // 将程序字节码附加到元数据后面
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Buffer(UiBuffer {
                authority: Some(authority.to_string()), // 预期 authority 字符串
                data: UiAccountData::Binary( // 预期 Base64 编码的程序数据
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );

        // 3. 测试 Buffer 状态 (无 authority - 理论上不常见，但为完整性测试)
        // "This case included for code completeness; in practice, a Buffer account will always have
        // authority_address.is_some()" -- 源码中的注释
        let bpf_loader_state = UpgradeableLoaderState::Buffer {
            authority_address: None, // authority 为 None
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap();
        account_data.extend_from_slice(&program);
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Buffer(UiBuffer {
                authority: None, // 预期 authority 为 None
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );

        // 4. 测试 Program 状态
        let programdata_address = Pubkey::new_unique(); // 创建一个随机的 ProgramData 地址
        let bpf_loader_state = UpgradeableLoaderState::Program {
            programdata_address, // 设置 ProgramData 地址
        };
        let account_data = serialize(&bpf_loader_state).unwrap();
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::Program(UiProgram {
                program_data: programdata_address.to_string(), // 预期 ProgramData 地址字符串
            })
        );

        // 5. 测试 ProgramData 状态 (有 upgrade authority)
        let authority = Pubkey::new_unique(); // 创建一个随机的升级管理者地址
        let slot = 42u64; // 任意的 slot 值
        let bpf_loader_state = UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address: Some(authority), // 设置升级管理者
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap(); // 序列化元数据
        account_data.extend_from_slice(&program); // 附加程序字节码
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::ProgramData(UiProgramData {
                slot, // 预期 slot 值
                authority: Some(authority.to_string()), // 预期升级管理者字符串
                data: UiAccountData::Binary( // 预期 Base64 编码的程序数据
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );

        // 6. 测试 ProgramData 状态 (无 upgrade authority)
        let bpf_loader_state = UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address: None, // 升级管理者为 None
        };
        let mut account_data = serialize(&bpf_loader_state).unwrap();
        account_data.extend_from_slice(&program);
        assert_eq!(
            parse_bpf_upgradeable_loader(&account_data).unwrap(),
            BpfUpgradeableLoaderAccountType::ProgramData(UiProgramData {
                slot,
                authority: None, // 预期升级管理者为 None
                data: UiAccountData::Binary(
                    BASE64_STANDARD.encode(&program),
                    UiAccountEncoding::Base64
                ),
            })
        );
    }
}
