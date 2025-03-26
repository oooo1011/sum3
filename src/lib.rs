use std::error::Error;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use rayon::prelude::*;
use crossbeam_channel::Sender;
use lru::LruCache;
use std::num::NonZeroUsize;

/// 缓存结构体
struct CombinationCache {
    cache: Mutex<LruCache<String, Vec<Vec<f64>>>>,
}

impl CombinationCache {
    fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap();
        CombinationCache {
            cache: Mutex::new(LruCache::new(cap)),
        }
    }

    fn get(&self, target: f64, tolerance: f64) -> Option<Vec<Vec<f64>>> {
        let key = format!("{:.2}_{:.2}", target, tolerance);
        self.cache.lock().unwrap().get(&key).cloned()
    }

    fn put(&self, target: f64, tolerance: f64, results: Vec<Vec<f64>>) {
        let key = format!("{:.2}_{:.2}", target, tolerance);
        self.cache.lock().unwrap().put(key, results);
    }
}

/// 查找数字组合的解
pub fn find_combinations(
    numbers: &[f64],
    target: f64,
    tolerance: f64,
    progress_tx: Option<crossbeam_channel::Sender<f64>>,
    max_length: usize,
    stop_flag: Arc<AtomicBool>,
) -> Vec<Vec<f64>> {
    // 初始化缓存(1GB容量，约可存储100万条记录)
    static CACHE: once_cell::sync::Lazy<CombinationCache> = once_cell::sync::Lazy::new(|| {
        CombinationCache::new(1_000_000)
    });

    // 检查缓存
    if let Some(cached) = CACHE.get(target, tolerance) {
        println!("从缓存中找到结果");
        return cached;
    }

    println!("输入数字: {:?}", numbers);
    println!("目标和: {}, 误差: {}, 最大长度: {}", target, tolerance, max_length);
    
    let results = Arc::new(Mutex::new(Vec::<Vec<f64>>::new()));
    let total = numbers.len();
    let max_results = 1000; // 限制最大结果数量
    let max_length = max_length; // 使用传入的参数值
    
    // 先排序数字以便更高效搜索
    let mut sorted_numbers = numbers.to_vec();
    sorted_numbers.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("排序后数字: {:?}", sorted_numbers);

    // 计算剩余数字的最大可能和
    fn max_remaining_sum(nums: &[f64], start: usize) -> f64 {
        nums[start..].iter().sum()
    }

    // 优化版回溯函数(带剪枝和并行计算)
    fn optimized_backtrack(
        nums: &[f64],
        target: f64,
        tolerance: f64,
        start: usize,
        path: &mut Vec<f64>,
        results: Arc<Mutex<Vec<Vec<f64>>>>,
        max_results: usize,
        max_length: usize,
        stop_flag: &AtomicBool,
    ) {
        // 检查停止标志
        if stop_flag.load(Ordering::Relaxed) {
            return;
        }

        // 检查当前路径是否满足条件
        let sum = path.iter().sum::<f64>();
        let diff = (sum - target).abs();
        
        if diff <= tolerance && !path.is_empty() {
            println!("找到解: {:?} (总和: {:.2}, 目标: {:.2}, 误差: {:.2})", path, sum, target, diff);
            if results.lock().unwrap().len() < max_results {
                results.lock().unwrap().push(path.clone());
            }
            return;
        }

        // 放宽剪枝条件: 仅保留结果数量限制和停止标志检查
        if results.lock().unwrap().len() >= max_results || 
           stop_flag.load(Ordering::Relaxed) {
            return;
        }

        // 并行处理分支
        (start..nums.len()).into_par_iter().for_each(|i| {
            if stop_flag.load(Ordering::Relaxed) {
                return;
            }
            
            let mut local_path = path.clone();
            local_path.push(nums[i]);
            optimized_backtrack(
                nums, target, tolerance, i + 1, 
                &mut local_path, results.clone(), max_results, max_length, stop_flag
            );
        });
    }

    let results = Arc::new(Mutex::new(Vec::new()));
    optimized_backtrack(
        &sorted_numbers, target, tolerance, 0,
        &mut Vec::new(), results.clone(), max_results, max_length, &stop_flag
    );
    
    let local_results = results.lock().unwrap().clone();
    
    let final_results = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
    
    // 存入缓存
    CACHE.put(target, tolerance, final_results.clone());
    final_results
}

/// 从CSV文件读取数字(支持单列和多列格式)
pub fn read_numbers_from_csv(path: &str) -> Result<Vec<f64>, Box<dyn Error>> {
    let content = std::fs::read_to_string(path)?;
    
    // 先尝试按行解析(单列CSV)
    let line_numbers: Vec<f64> = content
        .lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .collect();
    
    if !line_numbers.is_empty() {
        return Ok(line_numbers);
    }
    
    // 如果按行解析没有数据，尝试标准CSV解析(多列)
    let mut rdr = csv::Reader::from_path(path)?;
    let mut numbers = Vec::new();
    for result in rdr.records() {
        let record = result?;
        for field in record.iter() {
            if let Ok(num) = field.parse::<f64>() {
                numbers.push(num);
            }
        }
    }
    
    Ok(numbers)
}

/// 从TXT文件读取数字(每行一个数字)
pub fn read_numbers_from_txt(path: &str) -> Result<Vec<f64>, Box<dyn Error>> {
    let content = std::fs::read_to_string(path)?;
    let numbers = content
        .lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .collect();
    
    Ok(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_find_combinations() {
        let numbers = vec![1.0, 2.0, 3.0, 4.0];
        let target = 5.0;
        let tolerance = 0.1;
        let stop_flag = Arc::new(AtomicBool::new(false));
        
        // 测试精确匹配
        let result = find_combinations(&numbers, target, tolerance, None, 5, stop_flag.clone());
        assert!(result.iter().any(|r| (r.iter().sum::<f64>() - target).abs() <= tolerance));
        
        // 测试进度报告
        let (sender, receiver) = unbounded();
        find_combinations(&numbers, target, tolerance, Some(sender), 5, stop_flag.clone());
        assert!(receiver.try_recv().is_ok());
        
        // 测试边界情况
        let empty_result = find_combinations(&[], target, tolerance, None, 5, stop_flag);
        assert!(empty_result.is_empty());
    }

    #[test]
    fn test_read_numbers_from_csv() {
        let temp_file = std::env::temp_dir().join("test_numbers.csv");
        std::fs::write(&temp_file, "1.0\n2.0\n3.0").unwrap();
        
        let result = read_numbers_from_csv(temp_file.to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1.0, 2.0, 3.0]);
    }
}
