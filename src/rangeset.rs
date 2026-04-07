//! RangeSet - 数据范围集合，用于缓存管理
//!
//! RangeSet 用于记录一段连续数据的覆盖范围，格式为左闭右开区间：
//! [(0, 100), (200, 300)] 表示已缓存 0-99 和 200-299 的数据

use serde::{Deserialize, Serialize};

/// 数据范围区间（左闭右开）
pub type Range = (i64, i64);

/// RangeSet - 有序且不重叠的范围集合
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RangeSet(Vec<Range>);

impl RangeSet {
    /// 创建空的 RangeSet
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// 从已有的范围列表创建
    pub fn from_ranges(ranges: Vec<Range>) -> Self {
        let mut rs = Self(ranges);
        rs.normalize();
        rs
    }

    /// 添加一个范围
    pub fn insert(&mut self, start: i64, end: i64) {
        if start >= end {
            return;
        }
        self.0.push((start, end));
        self.normalize();
    }

    /// 添加一个范围区间
    pub fn insert_range(&mut self, range: Range) {
        self.insert(range.0, range.1);
    }

    /// 移除一个范围
    pub fn remove(&mut self, start: i64, end: i64) {
        if start >= end {
            return;
        }

        let mut new_ranges = Vec::new();
        for &(s, e) in &self.0 {
            if e <= start || s >= end {
                // 无交集，保留原范围
                if e > start && s < end {
                    // 完全覆盖，不保留
                } else {
                    new_ranges.push((s, e));
                }
            } else {
                // 有交集
                if s < start {
                    new_ranges.push((s, start));
                }
                if e > end {
                    new_ranges.push((end, e));
                }
            }
        }
        self.0 = new_ranges;
        self.normalize();
    }

    /// 检查是否包含某个位置
    pub fn contains(&self, pos: i64) -> bool {
        self.0.iter().any(|&(s, e)| s <= pos && pos < e)
    }

    /// 检查是否与某个范围有交集
    pub fn intersects(&self, start: i64, end: i64) -> bool {
        self.0.iter().any(|&(s, e)| s < end && e > start)
    }

    /// 获取所有范围
    pub fn ranges(&self) -> &[Range] {
        &self.0
    }

    /// 获取范围数量
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 获取覆盖的总数
    pub fn total_count(&self) -> i64 {
        self.0.iter().map(|&(s, e)| e - s).sum()
    }

    /// 合并相邻或重叠的范围
    fn normalize(&mut self) {
        if self.0.is_empty() {
            return;
        }

        // 按起始位置排序
        self.0.sort_by_key(|r| r.0);

        // 合并相邻和重叠的范围
        let mut merged = Vec::new();
        let mut current = self.0[0];

        for &(s, e) in &self.0[1..] {
            if s <= current.1 {
                // 合并
                current.1 = current.1.max(e);
            } else {
                merged.push(current);
                current = (s, e);
            }
        }
        merged.push(current);

        self.0 = merged;
    }

    /// 清空
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// 计算需要下载的范围（差集）
///
/// 返回 need 中有但 have 中没有的部分
pub fn rangeset_difference(have: &RangeSet, need: &RangeSet) -> Vec<Range> {
    let have_ranges = have.ranges();
    let need_ranges = need.ranges();

    let mut result = Vec::new();

    for &(ns, ne) in need_ranges {
        let mut start = ns;
        let end = ne;

        while start < end {
            // 找到第一个与 start 相交的 have 区间
            let mut found = false;
            for &(hs, he) in have_ranges {
                if he <= start {
                    continue;
                }
                if hs >= end {
                    break;
                }

                found = true;
                if hs > start {
                    // hs > start，中间有一段需要下载
                    result.push((start, hs.min(end)));
                }
                start = he.max(start);

                if start >= end {
                    break;
                }
            }

            if !found {
                // 没有相交的 have 区间，整段都需要下载
                result.push((start, end));
                break;
            }
        }
    }

    result
}

/// 计算两个 RangeSet 的并集
pub fn rangeset_union(a: &RangeSet, b: &RangeSet) -> RangeSet {
    let mut combined = a.ranges().to_vec();
    combined.extend_from_slice(b.ranges());
    RangeSet::from_ranges(combined)
}

/// 计算两个 RangeSet 的交集
pub fn rangeset_intersection(a: &RangeSet, b: &RangeSet) -> Vec<Range> {
    let mut result = Vec::new();

    for &(as_, ae) in a.ranges() {
        for &(bs, be) in b.ranges() {
            let start = as_.max(bs);
            let end = ae.min(be);
            if start < end {
                result.push((start, end));
            }
        }
    }

    result
}

/// 创建指定范围的目标 RangeSet
pub fn make_target_range(count: usize) -> RangeSet {
    RangeSet::from_ranges(vec![(0, count as i64)])
}

/// 从 ID 列表创建 RangeSet
pub fn from_ids(ids: &[i64]) -> RangeSet {
    if ids.is_empty() {
        return RangeSet::new();
    }

    let mut sorted = ids.to_vec();
    sorted.sort();

    let mut ranges = Vec::new();
    let mut start = sorted[0];
    let mut end = start + 1;

    for &id in &sorted[1..] {
        if id == end {
            end += 1;
        } else if id > end {
            ranges.push((start, end));
            start = id;
            end = id + 1;
        }
        // 如果 id < end，跳过（重复）
    }

    ranges.push((start, end));
    RangeSet::from_ranges(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_set_basic() {
        let mut rs = RangeSet::new();
        rs.insert(0, 100);
        rs.insert(200, 300);

        assert!(rs.contains(50));
        assert!(!rs.contains(150));
        assert!(rs.contains(250));
    }

    #[test]
    fn test_range_set_merge() {
        let mut rs = RangeSet::new();
        rs.insert(0, 100);
        rs.insert(90, 150); // 重叠，应该合并
        rs.insert(150, 200); // 相邻，应该合并

        assert_eq!(rs.len(), 1);
        assert!(rs.contains(50));
        assert!(rs.contains(100));
        assert!(rs.contains(150));
    }

    #[test]
    fn test_rangeset_difference() {
        let have = RangeSet::from_ranges(vec![(0, 50), (80, 100)]);
        let need = RangeSet::from_ranges(vec![(0, 100)]);

        let diff = rangeset_difference(&have, &need);

        // 需要下载的是 50-80
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0], (50, 80));
    }

    #[test]
    fn test_rangeset_difference_multiple() {
        let have = RangeSet::from_ranges(vec![(0, 30), (60, 70), (90, 100)]);
        let need = RangeSet::from_ranges(vec![(0, 100)]);

        let diff = rangeset_difference(&have, &need);

        // 需要下载 30-60 和 70-90
        assert_eq!(diff.len(), 2);
        assert_eq!(diff[0], (30, 60));
        assert_eq!(diff[1], (70, 90));
    }

    #[test]
    fn test_from_ids() {
        let ids = vec![1, 2, 3, 5, 6, 10];
        let rs = from_ids(&ids);

        assert_eq!(rs.len(), 3);
    }
}
