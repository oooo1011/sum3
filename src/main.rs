use eframe::egui;
use sum3_solver::{find_combinations, read_numbers_from_csv, read_numbers_from_txt};
use std::sync::{mpsc, Arc, Mutex};
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
    cancel_sender: Option<mpsc::Sender<()>>,
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
            cancel_sender: None,
        }
    }
}

impl eframe::App for Sum3App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("数字组合求解器");
            
            // 错误显示
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, err);
            }

            // 文件导入区域
            ui.horizontal(|ui| {
                if ui.button("导入CSV文件").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        match read_numbers_from_csv(path.to_str().unwrap()) {
                            Ok(nums) => {
                                self.numbers = nums;
                                self.status = format!("已导入 {} 个数字", self.numbers.len());
                                self.error = None;
                            }
                            Err(e) => self.error = Some(format!("导入错误: {}", e)),
                        }
                    }
                }
                
                if ui.button("导入TXT文件").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        match read_numbers_from_txt(path.to_str().unwrap()) {
                            Ok(nums) => {
                                self.numbers = nums;
                                self.status = format!("已导入 {} 个数字", self.numbers.len());
                                self.error = None;
                            }
                            Err(e) => self.error = Some(format!("导入错误: {}", e)),
                        }
                    }
                }
                
                ui.label(&self.status);
            });

            // 参数设置区域
            ui.horizontal(|ui| {
                ui.label("目标和:");
                ui.text_edit_singleline(&mut self.target);
                ui.label("误差范围:");
                ui.text_edit_singleline(&mut self.tolerance);
                ui.label("最大长度:");
                ui.text_edit_singleline(&mut self.max_length);
            });

            // 计算按钮
            ui.horizontal(|ui| {
                if ui.button("开始计算").clicked() && !self.computing {
                    self.start_computation(ctx.clone());
                }
                if ui.button("停止计算").clicked() && self.computing {
                    self.stop_computation();
                }
                ui.checkbox(&mut self.show_all, "显示所有解");
            });

            // 进度条
            ui.add(egui::ProgressBar::new(self.progress).text("计算进度"));

            // 结果显示区域
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
        let (cancel_tx, cancel_rx) = mpsc::channel();

        // 保存取消通道以便停止计算
        self.cancel_sender = Some(cancel_tx);

        let tx = Arc::new(Mutex::new(tx));
        
        // 启动计算线程
        let computation_thread = thread::spawn({
            let numbers = numbers.clone();
            let tx = tx.clone();
            move || {
                let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
                let (result_tx, result_rx) = mpsc::channel();
                
                // 计算线程
                thread::spawn(move || {
                    let results = find_combinations(
                        &numbers,
                        target,
                        tolerance,
                        Some(progress_tx),
                        max_length,
                    );
                    result_tx.send(results).unwrap();
                });

                // 进度更新线程
                thread::spawn({
                    let tx = tx.clone();
                    move || {
                        while let Ok(progress) = progress_rx.recv() {
                            if let Err(_) = tx.lock().unwrap().send(ComputationMessage::Progress(progress)) {
                                break;
                            }
                        }
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
            }
        });

        // 取消监听线程
        thread::spawn(move || {
            if cancel_rx.recv().is_ok() {
                computation_thread.thread().unpark();
            }
        });

        let ctx_clone = ctx.clone();
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        enum ComputationMessage {
            Progress(f64),
            Results(Vec<Vec<f64>>),
        }

        let shared_state = Arc::new(Mutex::new((
            self.results.clone(),
            self.progress,
            self.status.clone(),
            self.computing,
        )));

        let state_clone = shared_state.clone();
        thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                let mut state = state_clone.lock().unwrap();
                match msg {
                    ComputationMessage::Progress(p) => {
                        state.1 = p as f32;
                    }
                    ComputationMessage::Results(results) => {
                        state.0 = results;
                        state.1 = 1.0;
                        state.2 = format!("找到 {} 个解", state.0.len());
                        state.3 = false;
                    }
                }
                ctx_clone.request_repaint();
            }
        });

        // 在主线程中更新UI状态
        let state = shared_state.lock().unwrap();
        self.results = state.0.clone();
        self.progress = state.1;
        self.status = state.2.clone();
        self.computing = state.3;
    }

    fn stop_computation(&mut self) {
        if let Some(sender) = self.cancel_sender.take() {
            let _ = sender.send(());
            self.computing = false;
            self.status = "计算已停止".to_string();
        }
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
