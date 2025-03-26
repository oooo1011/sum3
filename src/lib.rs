use std::error::Error;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;
use crossbeam_channel::Sender;

/// 查找数字组合的解
pub fn find_combinations(
    numbers: &[f64],
    target: f64,
    tolerance: f64,
    progress_tx: Option<crossbeam_channel::Sender<f64>>,
    max_length: usize,
) -> Vec<Vec<f64>> {
    println!("输入数字: {:?}", numbers);
    println!("目标和: {}, 误差: {}, 最大长度: {}", target, tolerance, max_length);
    
    let results = Arc::new(Mutex::new(Vec::new()));
    let _total = numbers.len();
    let max_results = 1000; // 限制最大结果数量
    let max_length = max_length; // 使用传入的参数值
    
    // 先排序数字以便更高效搜索
    let mut sorted_numbers = numbers.to_vec();
    sorted_numbers.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("排序后数字: {:?}", sorted_numbers);

    // 并行回溯函数
    fn parallel_backtrack(
        nums: &[f64],
        target: f64,
        tolerance: f64,
        start: usize,
        path: &mut Vec<f64>,
        results: &Arc<Mutex<Vec<Vec<f64>>>>,
        max_results: usize,
        max_length: usize,
        sender: &Option<Sender<f64>>,
        total: usize,
        progress: &Arc<Mutex<usize>>,
    ) {
        // 检查当前路径是否满足条件
        let sum = path.iter().sum::<f64>();
        let diff = (sum - target).abs();
        
        // 允许单元素解
        if (diff <= tolerance && !path.is_empty()) || 
           (path.len() == 1 && diff <= tolerance) {
            println!("找到解: {:?} (总和: {:.2})", path, sum);
            let mut results = results.lock().unwrap();
            if results.len() < max_results {
                results.push(path.clone());
            }
        }

        if path.len() >= max_length || results.lock().unwrap().len() >= max_results {
            return;
        }

        // 并行处理分支
        (start..nums.len()).into_par_iter().for_each(|i| {
            // 更宽松的剪枝条件
            let current_sum = path.iter().sum::<f64>();
            if current_sum + nums[i] > target + tolerance {
                return;
            }

            let mut local_path = path.clone();
            local_path.push(nums[i]);
            
            // 批量更新进度(每100次更新一次)
            {
                let mut progress = progress.lock().unwrap();
                *progress += 1;
                
                // 报告进度(减少锁竞争)
                if *progress % 100 == 0 {
                    if let Some(s) = sender {
                        let _ = s.send(*progress as f64 / total as f64);
                    }
                }
            }

            // 提前终止检查
            if results.lock().unwrap().len() < max_results {
                parallel_backtrack(
                    nums, target, tolerance, i + 1,
                    &mut local_path, results, max_results, max_length,
                    sender, total, progress
                );
            }
        });
    }

    let progress = Arc::new(Mutex::new(0));
    // 简化进度计算
    let total = numbers.len();
    let mut path = Vec::new();
    
    parallel_backtrack(
        &sorted_numbers, target, tolerance, 0,
        &mut path, &results, max_results, max_length,
        &progress_tx, total, &progress
    );
    
    Arc::try_unwrap(results).unwrap().into_inner().unwrap()
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

    #[test]
    fn test_find_combinations() {
        let numbers = vec![1.0, 2.0, 3.0, 4.0];
        let target = 5.0;
        let tolerance = 0.1;
        
        // 测试精确匹配
        let result = find_combinations(&numbers, target, tolerance, None, 5);
        assert!(result.iter().any(|r| (r.iter().sum::<f64>() - target).abs() <= tolerance));
        
        // 测试进度报告
        let (sender, receiver) = unbounded();
        find_combinations(&numbers, target, tolerance, Some(sender), 5);
        assert!(receiver.try_recv().is_ok());
        
        // 测试边界情况
        let empty_result = find_combinations(&[], target, tolerance, None, 5);
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
