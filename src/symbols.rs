//! 外盘期货品种符号常量
//!
//! 新浪财经外盘期货合约代码列表。

/// 全部支持的外盘期货品种
pub const ALL_SYMBOLS: &[(&str, &str)] = &[
    // ===== 能源 =====
    ("hf_CL", "纽约原油"),
    ("hf_NG", "美国天然气"),
    ("hf_OIL", "布伦特原油"),
    // ===== 贵金属 =====
    ("hf_GC", "纽约黄金"),
    ("hf_SI", "纽约白银"),
    ("hf_XAU", "伦敦金（现货黄金）"),
    ("hf_XAG", "伦敦银（现货白银）"),
    ("hf_XPT", "纽约铂金"),
    ("hf_XPD", "纽约钯金"),
    // ===== 基本金属 =====
    ("hf_CAD", "伦铜"),
    ("hf_AHD", "伦铝"),
    ("hf_NID", "伦镍"),
    ("hf_PBD", "伦铅"),
    ("hf_SND", "伦锡"),
    ("hf_ZSD", "伦锌"),
    ("hf_HG", "美铜"),
    // ===== 农产品 =====
    ("hf_S", "美国大豆"),
    ("hf_W", "美国小麦"),
    ("hf_C", "美国玉米"),
    ("hf_SM", "美黄豆粉"),
    ("hf_BO", "美黄豆油"),
    ("hf_RS", "美国原糖"),
    ("hf_CT", "美国棉花"),
    // ===== 畜牧业 =====
    ("hf_LHC", "美瘦猪肉"),
    // ===== 指数/其他 =====
    ("hf_BTC", "比特币期货"),
    ("hf_EUA", "欧洲碳排放"),
    // ===== 亚洲商品 =====
    ("hf_FEF", "新加坡铁矿石"),
    ("hf_FCPO", "马棕油"),
    ("hf_RSS3", "日橡胶"),
];

/// 按类别分组的外盘期货品种
pub mod categories {

    /// 能源期货
    pub const ENERGY: &[(&str, &str)] = &[
        ("hf_CL", "纽约原油"),
        ("hf_NG", "美国天然气"),
        ("hf_OIL", "布伦特原油"),
    ];

    /// 贵金属期货
    pub const PRECIOUS_METALS: &[(&str, &str)] = &[
        ("hf_GC", "纽约黄金"),
        ("hf_SI", "纽约白银"),
        ("hf_XAU", "伦敦金（现货黄金）"),
        ("hf_XAG", "伦敦银（现货白银）"),
        ("hf_XPT", "纽约铂金"),
        ("hf_XPD", "纽约钯金"),
    ];

    /// 基本金属期货
    pub const BASE_METALS: &[(&str, &str)] = &[
        ("hf_CAD", "伦铜"),
        ("hf_AHD", "伦铝"),
        ("hf_NID", "伦镍"),
        ("hf_PBD", "伦铅"),
        ("hf_SND", "伦锡"),
        ("hf_ZSD", "伦锌"),
        ("hf_HG", "美铜"),
    ];

    /// 农产品期货
    pub const AGRICULTURE: &[(&str, &str)] = &[
        ("hf_S", "美国大豆"),
        ("hf_W", "美国小麦"),
        ("hf_C", "美国玉米"),
        ("hf_SM", "美黄豆粉"),
        ("hf_BO", "美黄豆油"),
        ("hf_RS", "美国原糖"),
        ("hf_CT", "美国棉花"),
    ];

    /// 畜牧业期货
    pub const LIVESTOCK: &[(&str, &str)] = &[("hf_LHC", "美瘦猪肉")];

    /// 其他期货
    pub const OTHER: &[(&str, &str)] = &[("hf_BTC", "比特币期货"), ("hf_EUA", "欧洲碳排放")];

    /// 亚洲商品期货
    pub const ASIA: &[(&str, &str)] = &[
        ("hf_FEF", "新加坡铁矿石"),
        ("hf_FCPO", "马棕油"),
        ("hf_RSS3", "日橡胶"),
    ];
}

/// 获取所有符号代码
///
/// # 示例
///
/// ```rust
/// use sina_quotes::symbols;
///
/// for (code, name) in symbols::ALL_SYMBOLS {
///     println!("{}: {}", code, name);
/// }
/// ```
pub fn all_codes() -> Vec<&'static str> {
    ALL_SYMBOLS.iter().map(|(code, _)| *code).collect()
}

/// 获取所有符号的名称和代码
pub fn all() -> &'static [(&'static str, &'static str)] {
    ALL_SYMBOLS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_symbols_count() {
        assert_eq!(ALL_SYMBOLS.len(), 29);
    }

    #[test]
    fn test_all_codes() {
        let codes = all_codes();
        assert!(codes.contains(&"hf_CL"));
        assert!(codes.contains(&"hf_GC"));
        assert!(codes.contains(&"hf_OIL"));
    }

    #[test]
    fn test_categories_complete() {
        let total = categories::ENERGY.len()
            + categories::PRECIOUS_METALS.len()
            + categories::BASE_METALS.len()
            + categories::AGRICULTURE.len()
            + categories::LIVESTOCK.len()
            + categories::OTHER.len()
            + categories::ASIA.len();
        assert_eq!(total, ALL_SYMBOLS.len());
    }
}
