// 文件功能说明：
// 这个文件负责解析 Solana 的地址查找表 (Address Lookup Table, ALT) 账户。
// 地址查找表是 Solana 中一项重要的功能，它允许开发者将一组常用的账户地址（公钥）存储在一个链上的表中。
// 在构造交易时，可以直接通过索引来引用这些地址，而不是每次都包含完整的32字节公钥。
// 这样做的好处是可以显著减小交易的体积，从而降低交易费用，并允许在单个交易中包含更多的指令和账户引用。
// 本文件中的代码会将 ALT 账户的二进制数据反序列化为易于理解的结构体，方便钱包、浏览器等工具展示其内容。

use {
    // 从当前 crate (account-decoder) 的 `parse_account_data` 模块中导入 `ParsableAccount` 和 `ParseAccountError`。
    // `ParsableAccount` 是一个枚举，列出了所有可以被解析的账户类型。
    // `ParseAccountError` 是一个枚举，定义了在解析过程中可能发生的错误。
    crate::parse_account_data::{ParsableAccount, ParseAccountError},
    // 从 `solana_address_lookup_table_interface` crate 中导入 `AddressLookupTable` 结构体。
    // 这个结构体定义了地址查找表账户在链上存储的原始格式。
    solana_address_lookup_table_interface::state::AddressLookupTable,
    // 从 `solana_instruction` crate 中导入 `InstructionError` 枚举。
    // 这个枚举包含了指令处理过程中可能发生的各种错误，例如账户未初始化。
    solana_instruction::error::InstructionError,
};

// 函数 `parse_address_lookup_table`：
// 功能：解析地址查找表账户的原始二进制数据。
// 参数：
//   - `data`: `&[u8]`，一个字节切片，代表地址查找表账户的原始数据。`&[u8]` 表示对一个 u8 数组的不可变借用。
// 返回值：
//   - `Result<LookupTableAccountType, ParseAccountError>`:
//     - 如果解析成功，返回 `Ok(LookupTableAccountType)`，其中 `LookupTableAccountType` 是一个枚举，
//       包含了已初始化并成功解析的查找表信息 (`LookupTable`) 或表示账户未初始化 (`Uninitialized`)。
//     - 如果解析失败，返回 `Err(ParseAccountError)`，其中 `ParseAccountError` 包含了具体的错误信息。
// 设计思路：
//   1. 尝试使用 `AddressLookupTable::deserialize(data)` 从原始数据中反序列化出 `AddressLookupTable` 结构体。
//      `deserialize` 方法会按照 `AddressLookupTable` 定义的链上布局来解析字节。
//   2. 如果反序列化成功（`.map(...)`）：
//      - 将原始的 `AddressLookupTable` 转换为用户友好的 `UiLookupTable` 格式（通过 `.into()`，因为我们为 `UiLookupTable` 实现了 `From<AddressLookupTable>`）。
//      - 然后将这个 `UiLookupTable` 包装在 `LookupTableAccountType::LookupTable` 枚举成员中并返回。
//   3. 如果反序列化失败（`.or_else(|err| ...)`）：
//      - 检查错误类型 `err`：
//        - 如果错误是 `InstructionError::UninitializedAccount`，这表示账户存在但尚未被初始化（即还没有写入有效的数据）。
//          在这种情况下，我们认为这是一种有效的状态，并返回 `Ok(LookupTableAccountType::Uninitialized)`。
//        - 对于任何其他类型的反序列化错误，我们认为这是一个真正的解析失败，
//          并返回 `Err(ParseAccountError::AccountNotParsable(ParsableAccount::AddressLookupTable))`，
//          指明这个地址查找表账户无法被正确解析。
pub fn parse_address_lookup_table(
    data: &[u8],
) -> Result<LookupTableAccountType, ParseAccountError> {
    // 尝试从原始数据反序列化 AddressLookupTable。
    AddressLookupTable::deserialize(data)
        .map(|address_lookup_table| { // 如果成功...
            // 将原始的 AddressLookupTable 转换为 UiLookupTable，然后包装在 LookupTableAccountType::LookupTable 中。
            LookupTableAccountType::LookupTable(address_lookup_table.into())
        })
        .or_else(|err| { // 如果失败 (err)...
            // `match` 表达式用于根据 err 的具体类型执行不同的逻辑。
            match err {
                // 如果错误是因为账户未初始化...
                InstructionError::UninitializedAccount => Ok(LookupTableAccountType::Uninitialized),
                // 对于其他所有错误...
                _ => Err(ParseAccountError::AccountNotParsable( // 返回一个表示账户不可解析的错误。
                    ParsableAccount::AddressLookupTable, // 指明是地址查找表类型的账户。
                )),
            }
        })
}

// 枚举 `LookupTableAccountType`:
// 表示地址查找表账户可能的两种状态：未初始化或已初始化的查找表。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`:
//   - `Debug`: 允许使用 `{:?}` 格式化打印，方便调试。
//   - `Serialize`, `Deserialize`: 自动派生 `serde` 的序列化和反序列化能力。
//   - `PartialEq`, `Eq`: 允许比较此枚举的实例是否相等。
// `#[serde(rename_all = "camelCase", tag = "type", content = "info")]`:
//   - `rename_all = "camelCase"`: 在序列化为 JSON 时，枚举成员名和字段名将转换为驼峰式命名。
//   - `tag = "type"`: 在序列化时，会添加一个名为 "type" 的字段，其值为枚举成员的名称（例如 "uninitialized" 或 "lookupTable"）。
//   - `content = "info"`: 对于带有数据的枚举成员（如此处的 `LookupTable(UiLookupTable)`），其包含的数据会序列化到一个名为 "info" 的字段中。
// 例如，`LookupTableAccountType::LookupTable(ui_table)` 会序列化为类似：
//   `{ "type": "lookupTable", "info": { ... ui_table 的内容 ... } }`
// 而 `LookupTableAccountType::Uninitialized` 会序列化为：
//   `{ "type": "uninitialized" }`
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum LookupTableAccountType {
    Uninitialized,        // 表示地址查找表账户尚未初始化。
    LookupTable(UiLookupTable), // 表示一个已初始化的地址查找表，包含其详细信息 `UiLookupTable`。
}

// 结构体 `UiLookupTable`:
// 用于以用户友好的方式显示地址查找表的详细信息。
// 它的字段对应于链上 `AddressLookupTable` 结构中的元数据和地址列表，但通常会转换为字符串等更易读的格式。
// `#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]`: 派生常用的特性。
// `#[serde(rename_all = "camelCase")]`: 序列化为 JSON 时字段名使用驼峰式。
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiLookupTable {
    // `deactivation_slot`: 地址查找表被设置为非活动状态的 slot (槽)。
    // Slot 是 Solana 中时间的单位，大约对应一个区块产生的时间。
    // 一旦表被停用，就不能再向其中添加新的地址。
    // 这里用 `String` 类型表示，因为 `u64` 类型的 slot 号码可能非常大。
    pub deactivation_slot: String,

    // `last_extended_slot`: 地址查找表最后一次被扩展（即添加新地址）的 slot。
    pub last_extended_slot: String,

    // `last_extended_slot_start_index`: 在 `last_extended_slot` 中，新添加的地址开始的索引位置。
    // 一个查找表可以分多次扩展。
    pub last_extended_slot_start_index: u8,

    // `authority`: 可选的，管理此地址查找表的账户的公钥。
    // 只有 authority 才能扩展或停用查找表。如果为 `None`，则该表是不可变的。
    // `#[serde(skip_serializing_if = "Option::is_none")]`: 如果 `authority` 字段是 `None`，则在序列化为 JSON 时会跳过此字段。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,

    // `addresses`: 一个 `Vec<String>`，存储查找表中所有地址的字符串表示。
    // `Vec<T>` 是 Rust 中的动态数组（向量）。
    pub addresses: Vec<String>,
}

// 实现 `From<AddressLookupTable<'_>> for UiLookupTable`:
// 这个 `impl` 块为 `UiLookupTable` 结构体实现了 `From` 特性。
// `From<T>` 特性允许我们定义如何从类型 `T`（这里是 `AddressLookupTable`）创建一个 `Self`（这里是 `UiLookupTable`）的实例。
// 这使得类型转换非常方便，例如可以使用 `ui_table = UiLookupTable::from(raw_table)` 或 `ui_table = raw_table.into()`。
// 参数 `address_lookup_table: AddressLookupTable<'_>`:
//   - `AddressLookupTable` 是从链上直接反序列化得到的原始结构。
//   - `<'_>` 是一个生命周期参数，表示 `AddressLookupTable` 可能借用了某些数据（例如，它的 `addresses` 字段可能是对原始数据切片的借用 `Cow<'_, [Pubkey]>`）。
//     在 `From` 实现中，我们通常会获取这些数据的副本或将其转换为拥有的类型（如 `String`），所以 `UiLookupTable` 不再有生命周期参数。
impl From<AddressLookupTable<'_>> for UiLookupTable {
    fn from(address_lookup_table: AddressLookupTable) -> Self {
        // `Self { ... }` 是构造当前结构体实例的语法。
        Self {
            // 将 u64 类型的 slot 号转换为 String。
            deactivation_slot: address_lookup_table.meta.deactivation_slot.to_string(),
            last_extended_slot: address_lookup_table.meta.last_extended_slot.to_string(),
            last_extended_slot_start_index: address_lookup_table
                .meta
                .last_extended_slot_start_index,
            // `address_lookup_table.meta.authority` 是一个 `Option<Pubkey>`。
            // `.map(|authority| authority.to_string())`：
            //   如果 `authority` 是 `Some(pubkey)`，则对 `pubkey` 调用 `.to_string()` 转换为字符串，结果是 `Some(String)`。
            //   如果 `authority` 是 `None`，则结果仍然是 `None`。
            authority: address_lookup_table
                .meta
                .authority
                .map(|authority| authority.to_string()),
            // `address_lookup_table.addresses` 是一个 `Cow<'_, [Pubkey]>`。
            // `Cow` (Clone-on-Write) 是一种智能指针，可以表示借用的数据或拥有的数据。
            // `.iter()`: 获取地址列表的迭代器。
            // `.map(|address| address.to_string())`: 对每个 `Pubkey` 类型的地址调用 `.to_string()` 转换为字符串。
            // `.collect()`: 将迭代器产生的所有字符串收集到一个新的 `Vec<String>` 中。
            addresses: address_lookup_table
                .addresses
                .iter()
                .map(|address| address.to_string())
                .collect(),
        }
    }
}

// 测试模块
// `#[cfg(test)]` 属性宏表示下面的 `mod test` 模块只在执行 `cargo test` 命令时编译和运行。
#[cfg(test)]
mod test {
    // 导入父模块（即本文件 `parse_address_lookup_table.rs`）中的所有公共项。
    use super::*;
    // 从 `solana_address_lookup_table_interface` 中导入测试需要用到的 `LookupTableMeta` 和常量 `LOOKUP_TABLE_META_SIZE`。
    use solana_address_lookup_table_interface::state::{LookupTableMeta, LOOKUP_TABLE_META_SIZE};
    // 导入 `Pubkey`。
    use solana_pubkey::Pubkey;
    // 导入 `std::borrow::Cow`，因为原始的 `AddressLookupTable` 的 `addresses` 字段使用它。
    use std::borrow::Cow;

    // 测试函数 `test_parse_address_lookup_table`
    // `#[test]` 宏标记这是一个单元测试函数。
    #[test]
    fn test_parse_address_lookup_table() {
        // 1. 准备一个有效的 AddressLookupTable 数据
        let authority = Pubkey::new_unique(); // 创建一个唯一的随机公钥作为 authority。
        let deactivation_slot = 1u64;
        let last_extended_slot = 2u64;
        let last_extended_slot_start_index = 3u8;

        // 创建查找表的元数据。
        let lookup_table_meta = LookupTableMeta {
            deactivation_slot,
            last_extended_slot,
            last_extended_slot_start_index,
            authority: Some(authority),
            ..LookupTableMeta::default() // `..Default::default()` 表示其余字段使用其默认值。
        };

        let num_addresses = 42; // 表中包含的地址数量。
        let mut addresses = Vec::with_capacity(num_addresses); // 创建一个有足够容量的 Vec。
        // 用随机生成的唯一公钥填充地址列表。
        addresses.resize_with(num_addresses, Pubkey::new_unique);

        // 创建一个原始的 AddressLookupTable 实例。
        // `Cow::Owned(addresses)` 表示 `addresses` 字段现在拥有这些数据 (而不是借用)。
        let lookup_table = AddressLookupTable {
            meta: lookup_table_meta,
            addresses: Cow::Owned(addresses),
        };

        // 将 `lookup_table` 序列化为字节数据，模拟链上存储的数据。
        // `serialize_for_tests` 是一个辅助函数，用于测试目的的序列化。
        let lookup_table_data = AddressLookupTable::serialize_for_tests(lookup_table).unwrap();

        // 2. 调用解析函数并进行断言
        // `.unwrap()` 用于获取 `Result` 中的 `Ok` 值，如果解析失败则测试会 panic。
        let parsing_result = parse_address_lookup_table(&lookup_table_data).unwrap();

        // `if let` 是一种方便的模式匹配方式，用于当只关心一种或少数几种模式时。
        // 这里检查 `parsing_result` 是否是 `LookupTableAccountType::LookupTable` 成员，
        // 如果是，则将其中的 `ui_lookup_table` 解构出来。
        if let LookupTableAccountType::LookupTable(ui_lookup_table) = parsing_result {
            // `assert_eq!` 宏断言两个表达式的值相等。
            assert_eq!(
                ui_lookup_table.deactivation_slot,
                deactivation_slot.to_string()
            );
            assert_eq!(
                ui_lookup_table.last_extended_slot,
                last_extended_slot.to_string()
            );
            assert_eq!(
                ui_lookup_table.last_extended_slot_start_index,
                last_extended_slot_start_index
            );
            assert_eq!(ui_lookup_table.authority, Some(authority.to_string()));
            assert_eq!(ui_lookup_table.addresses.len(), num_addresses);
        }

        // 3. 测试未初始化账户的情况
        // `LOOKUP_TABLE_META_SIZE` 是地址查找表元数据部分的固定大小。
        // 创建一个全零的字节数组，其长度等于元数据大小，模拟一个未初始化的（但已分配空间）账户。
        // 解析这种数据应该返回 `LookupTableAccountType::Uninitialized`。
        assert_eq!(
            parse_address_lookup_table(&[0u8; LOOKUP_TABLE_META_SIZE]).unwrap(),
            LookupTableAccountType::Uninitialized
        );

        // 4. 测试无效数据的情况
        // 传入一个空字节切片，这不构成一个有效的地址查找表账户，应该导致解析错误。
        // `.is_err()` 检查 `Result` 是否为 `Err` 变体。
        assert!(parse_address_lookup_table(&[]).is_err());
    }
}
