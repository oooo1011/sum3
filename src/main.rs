use eframe::egui;
use sum3_solver::{find_combinations, read_numbers_from_csv, read_numbers_from_txt};
use std::sync::{mpsc, Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;

struct Sum3App {
    numbers: Vec<f64>,
    target: String,
    tolerance: String,
    max_length: String,
    results: Vec<Vec<f64>>,
    progress: f32,
    status: String,
    computing: bool,
    show_all: bool,
    error: Option<String>,
    stop_flag: Arc<AtomicBool>,
    shared_state: Arc<Mutex<(Vec<Vec<f64>>, f32, String, bool)>>,
}

impl Default for Sum3App {
    fn default() -> Self {
        Self {
            numbers: Vec::new(),
            target: "10.0".to_string(),
            tolerance: "0.1".to_string(),
            max_length: "5".to_string(),
            results: Vec::new(),
            progress: 0.0,
            status: "就绪".to_string(),
            computing: false,
            show_all: false,
            error: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            shared_state: Arc::new(Mutex::new((Vec::new(), 0.0, "就绪".to_string(), false))),
        }
    }
}

impl eframe::App for Sum3App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 同步共享状态并立即释放锁
        {
            let state = self.shared_state.lock().unwrap();
            self.results = state.0.clone();
            self.progress = state.1;
            self.status = state.2.clone();
            self.computing = state.3;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("数字组合求解器");
            
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, err);
            }

            ui.horizontal(|ui| {
                if ui.button("导入数字文件").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        let path_str = path.to_str().unwrap();
                        let result = if path_str.ends_with(".csv") {
                            read_numbers_from_csv(path_str)
                        } else {
                            read_numbers_from_txt(path_str)
                        };
                        
                        match result {
                            Ok(nums) => {
                                self.numbers = nums;
                                println!("导入的数字: {:?}", self.numbers);  // 添加导入数字日志
                                self.status = format!("已导入 {} 个数字", self.numbers.len());
                                self.error = None;
                                // 强制更新UI状态
                                let mut state = self.shared_state.lock().unwrap();
                                state.2 = self.status.clone();
                                ctx.request_repaint();
                            }
                            Err(e) => {
                                self.error = Some(format!("导入错误: {}", e));
                            }
                        }
                    }
                }
                
                ui.label(&self.status);
            });

            ui.horizontal(|ui| {
                ui.label("目标和:");
                ui.text_edit_singleline(&mut self.target);
                ui.label("误差范围:");
                ui.text_edit_singleline(&mut self.tolerance);
                ui.label("最大长度:");
                ui.text_edit_singleline(&mut self.max_length);
            });

            ui.horizontal(|ui| {
                if ui.button("开始计算").clicked() && !self.computing {
                    self.start_computation(ctx.clone());
                }
                if ui.button("停止计算").clicked() && self.computing {
                    self.stop_computation();
                    println!("用户点击了停止按钮");
                }
                ui.checkbox(&mut self.show_all, "显示所有解");
            });

            ui.add(egui::ProgressBar::new(self.progress).text("计算进度"));

            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, res) in self.results.iter().enumerate() {
                    if !self.show_all && i >= 1 {
                        break;
                    }
                    let sum = res.iter().sum::<f64>();
                    ui.label(format!(
                        "解 {}: {:?} (总和: {:.2})",
                        i + 1,
                        res,
                        sum
                    ));
                }
            });
        });
    }
}

impl Sum3App {
    fn start_computation(&mut self, ctx: egui::Context) {
        let target = match self.target.parse::<f64>() {
            Ok(t) => t,
            Err(_) => {
                self.error = Some("无效的目标值".to_string());
                return;
            }
        };
        
        let tolerance = match self.tolerance.parse::<f64>() {
            Ok(t) => t,
            Err(_) => {
                self.error = Some("无效的误差范围".to_string());
                return;
            }
        };
        
        let max_length = match self.max_length.parse::<usize>() {
            Ok(m) => m,
            Err(_) => {
                self.error = Some("无效的最大长度".to_string());
                return;
            }
        };

        if self.numbers.is_empty() {
            self.error = Some("请先导入数字".to_string());
            return;
        }

        self.computing = true;
        self.results.clear();
        self.progress = 0.0;
        self.status = "计算中...".to_string();
        self.error = None;

        let numbers = self.numbers.clone();
        let (tx, rx) = mpsc::channel();
        self.stop_flag.store(false, Ordering::Relaxed);

        let tx = Arc::new(Mutex::new(tx));
        let stop_flag = self.stop_flag.clone();
        let shared_state = self.shared_state.clone();
        
        // 创建进度通道
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = mpsc::channel();

        // 计算线程
        thread::spawn({
            let numbers = numbers.clone();
            let stop_flag = stop_flag.clone();
            move || {
                println!("计算线程启动");
                let results = find_combinations(
                    &numbers,
                    target,
                    tolerance,
                    Some(progress_tx.clone()),  // 确保通道不被移动
                    max_length,
                    stop_flag,
                );
                // 显式关闭进度通道
                drop(progress_tx);
                println!("计算完成，找到{}个解", results.len());
                result_tx.send(results).unwrap();
            }
        });

        // 进度更新线程
        thread::spawn({
            let tx = tx.clone();
            move || {
                println!("进度线程启动");
                while let Ok(progress) = progress_rx.recv() {
                    println!("收到原始进度: {}", progress);
                    let clamped_progress = progress.clamp(0.0, 1.0);
                    println!("发送进度更新: {:.2}%", clamped_progress * 100.0);
                    if let Err(_) = tx.lock().unwrap().send(ComputationMessage::Progress(clamped_progress)) {
                        println!("进度通道已关闭");
                        break;
                    }
                }
                println!("进度线程结束");
            }
        });

        // 结果接收线程
        thread::spawn({
            let tx = tx.clone();
            move || {
                if let Ok(results) = result_rx.recv() {
                    let _ = tx.lock().unwrap().send(ComputationMessage::Results(results));
                }
            }
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        enum ComputationMessage {
            Progress(f64),
            Results(Vec<Vec<f64>>),
        }

        thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                let mut state = shared_state.lock().unwrap();
                match msg {
                    ComputationMessage::Progress(p) => {
                        state.1 = p as f32;
                        println!("进度更新: {:.2}%", p * 100.0);
                    }
                    ComputationMessage::Results(results) => {
                        state.0 = results;
                        state.1 = 1.0;
                        state.2 = format!("找到 {} 个解", state.0.len());
                        state.3 = false;
                    }
                }
                ctx.request_repaint();
            }
        });
    }

    fn stop_computation(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        let mut state = self.shared_state.lock().unwrap();
        state.3 = false;
        state.2 = "计算已停止".to_string();
        self.computing = false;
        self.status = "计算已停止".to_string();
        println!("计算已停止");
    }
}

fn main() {
    let options = eframe::NativeOptions::default();
    if let Err(e) = eframe::run_native(
        "数字组合求解器",
        options,
        Box::new(|cc| {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "noto_serif_cjk_sc".to_owned(),
                egui::FontData::from_static(include_bytes!("../fonts/NotoSerifCJKsc-Regular.otf")).into(),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "noto_serif_cjk_sc".to_owned());
            
            let mut style = egui::Style::default();
            style.text_styles.insert(
                egui::TextStyle::Heading,
                egui::FontId::new(24.0, egui::FontFamily::Name("noto_serif_cjk_sc".into())),
            );
            style.text_styles.insert(
                egui::TextStyle::Body,
                egui::FontId::new(16.0, egui::FontFamily::Name("noto_serif_cjk_sc".into())),
            );
            
            cc.egui_ctx.set_fonts(fonts);
            cc.egui_ctx.set_style(style);
            
            Ok(Box::new(Sum3App::default()))
        }),
    ) {
        eprintln!("应用程序错误: {}", e);
    }
}
