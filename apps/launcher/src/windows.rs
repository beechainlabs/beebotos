use std::cell::RefCell;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::{fs, thread};

use native_windows_gui as nwg;
use nwg::NativeUi;

use crate::{read_env_file, write_env_file, EnvConfig};

const WEB_URL: &str = "http://localhost:8090";

#[derive(Default)]
pub struct LauncherApp {
    app_dir: PathBuf,
    pending: RefCell<Option<Receiver<TaskResult>>>,

    window: nwg::Window,
    status_label: nwg::Label,
    text_key_label: nwg::Label,
    text_key_input: nwg::TextInput,
    image_key_label: nwg::Label,
    image_key_input: nwg::TextInput,
    video_key_label: nwg::Label,
    video_key_input: nwg::TextInput,
    reuse_key_button: nwg::Button,
    save_button: nwg::Button,
    start_button: nwg::Button,
    stop_button: nwg::Button,
    restart_button: nwg::Button,
    open_web_button: nwg::Button,
    refresh_button: nwg::Button,
    logs_button: nwg::Button,
    output_label: nwg::Label,
    output_box: nwg::TextBox,
    result_notice: nwg::Notice,
}

struct LauncherAppUi {
    inner: Rc<LauncherApp>,
    default_handler: RefCell<Option<nwg::EventHandler>>,
}

struct TaskResult {
    title: String,
    body: String,
    success: bool,
}

impl LauncherApp {
    fn env_path(&self) -> PathBuf {
        self.app_dir.join(".env")
    }

    fn current_config(&self) -> EnvConfig {
        EnvConfig {
            text_model_key: self.text_key_input.text().trim().to_owned(),
            image_generation_key: self.image_key_input.text().trim().to_owned(),
            video_generation_key: self.video_key_input.text().trim().to_owned(),
        }
    }

    fn secret_values(&self) -> Vec<String> {
        let config = self.current_config();
        vec![
            config.text_model_key,
            config.image_generation_key,
            config.video_generation_key,
        ]
    }

    fn save_config(&self) {
        match write_env_file(&self.env_path(), &self.current_config()) {
            Ok(()) => {
                self.status_label.set_text("服务状态：配置已保存");
                self.output_box.set_text_unix2dos(
                    "配置已保存到安装目录 .env。\nBEE_ALLOW_NETWORK=1 已写入。\n密钥不会写入 \
                     config/beebotos.toml。",
                );
            }
            Err(err) => {
                self.status_label.set_text("服务状态：配置保存失败");
                self.output_box
                    .set_text_unix2dos(&format!("保存 .env 失败：{err}"));
            }
        }
    }

    fn reuse_key(&self) {
        let key = self.text_key_input.text();
        if key.trim().is_empty() {
            self.status_label.set_text("服务状态：请先填写文本模型 Key");
            self.output_box
                .set_text_unix2dos("请先在“文本模型 Key”中填写火山方舟 Key。");
            return;
        }
        self.image_key_input.set_text(&key);
        self.video_key_input.set_text(&key);
        self.status_label.set_text("服务状态：已复用 Key");
        self.output_box
            .set_text_unix2dos("已将文本模型 Key 复制到图片生成和视频生成 Key。");
    }

    fn run_service_action(&self, action: &'static str, title: &'static str) {
        let app_dir = self.app_dir.clone();
        let secrets = self.secret_values();
        self.spawn_task(title, move || {
            run_powershell_action(&app_dir, action, title, &secrets)
        });
    }

    fn refresh_status(&self) {
        self.run_service_action("status", "刷新状态");
    }

    fn open_web(&self) {
        let app_dir = self.app_dir.clone();
        self.spawn_task("打开 Web", move || open_web(&app_dir));
    }

    fn view_logs(&self) {
        let app_dir = self.app_dir.clone();
        let secrets = self.secret_values();
        self.spawn_task("查看日志", move || read_logs(&app_dir, &secrets));
    }

    fn spawn_task<F>(&self, title: &'static str, task: F)
    where
        F: FnOnce() -> TaskResult + Send + 'static,
    {
        if self.pending.borrow().is_some() {
            return;
        }

        self.set_busy(true);
        self.status_label
            .set_text(&format!("服务状态：正在执行 {title}"));
        self.output_box
            .set_text_unix2dos(&format!("{title} 中，请稍候..."));

        let (tx, rx) = mpsc::channel();
        *self.pending.borrow_mut() = Some(rx);
        let notice = self.result_notice.sender();
        thread::spawn(move || {
            let result = task();
            let _ = tx.send(result);
            notice.notice();
        });
    }

    fn finish_task(&self) {
        let Some(rx) = self.pending.borrow_mut().take() else {
            return;
        };

        match rx.try_recv() {
            Ok(result) => {
                let status = if result.success { "完成" } else { "失败" };
                self.status_label
                    .set_text(&format!("服务状态：{}{}", result.title, status));
                self.output_box.set_text_unix2dos(&result.body);
                self.set_busy(false);
            }
            Err(TryRecvError::Empty) => {
                *self.pending.borrow_mut() = Some(rx);
            }
            Err(TryRecvError::Disconnected) => {
                self.status_label.set_text("服务状态：操作失败");
                self.output_box.set_text_unix2dos("后台任务异常结束。");
                self.set_busy(false);
            }
        }
    }

    fn set_busy(&self, busy: bool) {
        let enabled = !busy;
        self.start_button.set_enabled(enabled);
        self.stop_button.set_enabled(enabled);
        self.restart_button.set_enabled(enabled);
        self.open_web_button.set_enabled(enabled);
        self.refresh_button.set_enabled(enabled);
        self.logs_button.set_enabled(enabled);
    }

    fn exit(&self) {
        nwg::stop_thread_dispatch();
    }
}

impl nwg::NativeUi<LauncherAppUi> for LauncherApp {
    fn build_ui(mut data: LauncherApp) -> Result<LauncherAppUi, nwg::NwgError> {
        use nwg::Event as E;

        let env_path = data.env_path();
        let (initial_config, initial_output) = match read_env_file(&env_path) {
            Ok(config) => (
                config,
                format!("应用目录：{}\n.env 已就绪。", data.app_dir.display()),
            ),
            Err(err) => (
                EnvConfig::default(),
                format!(
                    "应用目录：{}\n读取 .env 失败：{err}",
                    data.app_dir.display()
                ),
            ),
        };

        nwg::Window::builder()
            .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
            .size((760, 620))
            .position((300, 180))
            .title("BeeBotOS Launcher")
            .build(&mut data.window)?;

        nwg::Label::builder()
            .text("服务状态：未刷新")
            .size((700, 26))
            .position((20, 20))
            .parent(&data.window)
            .build(&mut data.status_label)?;

        build_label(&data.window, &mut data.text_key_label, "文本模型 Key", 60)?;
        build_key_input(
            &data.window,
            &mut data.text_key_input,
            &initial_config.text_model_key,
            56,
            true,
        )?;

        build_label(&data.window, &mut data.image_key_label, "图片生成 Key", 100)?;
        build_key_input(
            &data.window,
            &mut data.image_key_input,
            &initial_config.image_generation_key,
            96,
            false,
        )?;

        build_label(&data.window, &mut data.video_key_label, "视频生成 Key", 140)?;
        build_key_input(
            &data.window,
            &mut data.video_key_input,
            &initial_config.video_generation_key,
            136,
            false,
        )?;

        build_button(
            &data.window,
            &mut data.reuse_key_button,
            "同一个火山方舟 Key 复用到三项",
            (160, 178),
            (260, 30),
        )?;
        build_button(
            &data.window,
            &mut data.save_button,
            "保存配置",
            (430, 178),
            (120, 30),
        )?;

        build_button(
            &data.window,
            &mut data.start_button,
            "启动",
            (20, 230),
            (95, 32),
        )?;
        build_button(
            &data.window,
            &mut data.stop_button,
            "停止",
            (125, 230),
            (95, 32),
        )?;
        build_button(
            &data.window,
            &mut data.restart_button,
            "重启",
            (230, 230),
            (95, 32),
        )?;
        build_button(
            &data.window,
            &mut data.open_web_button,
            "打开 Web",
            (335, 230),
            (105, 32),
        )?;
        build_button(
            &data.window,
            &mut data.refresh_button,
            "刷新状态",
            (450, 230),
            (105, 32),
        )?;
        build_button(
            &data.window,
            &mut data.logs_button,
            "查看日志",
            (565, 230),
            (105, 32),
        )?;

        nwg::Label::builder()
            .text("输出")
            .size((700, 24))
            .position((20, 285))
            .parent(&data.window)
            .build(&mut data.output_label)?;

        nwg::TextBox::builder()
            .text(&initial_output.replace('\n', "\r\n"))
            .size((700, 270))
            .position((20, 315))
            .readonly(true)
            .flags(
                nwg::TextBoxFlags::VISIBLE
                    | nwg::TextBoxFlags::VSCROLL
                    | nwg::TextBoxFlags::AUTOVSCROLL,
            )
            .parent(&data.window)
            .build(&mut data.output_box)?;

        nwg::Notice::builder()
            .parent(&data.window)
            .build(&mut data.result_notice)?;

        let ui = LauncherAppUi {
            inner: Rc::new(data),
            default_handler: Default::default(),
        };

        let evt_ui = Rc::downgrade(&ui.inner);
        let handle_events = move |evt, _evt_data, handle| {
            if let Some(ui) = evt_ui.upgrade() {
                match evt {
                    E::OnButtonClick if &handle == &ui.reuse_key_button => ui.reuse_key(),
                    E::OnButtonClick if &handle == &ui.save_button => ui.save_config(),
                    E::OnButtonClick if &handle == &ui.start_button => {
                        ui.run_service_action("start", "启动服务")
                    }
                    E::OnButtonClick if &handle == &ui.stop_button => {
                        ui.run_service_action("stop", "停止服务")
                    }
                    E::OnButtonClick if &handle == &ui.restart_button => {
                        ui.run_service_action("restart", "重启服务")
                    }
                    E::OnButtonClick if &handle == &ui.open_web_button => ui.open_web(),
                    E::OnButtonClick if &handle == &ui.refresh_button => ui.refresh_status(),
                    E::OnButtonClick if &handle == &ui.logs_button => ui.view_logs(),
                    E::OnNotice if &handle == &ui.result_notice => ui.finish_task(),
                    E::OnWindowClose if &handle == &ui.window => ui.exit(),
                    _ => {}
                }
            }
        };

        *ui.default_handler.borrow_mut() = Some(nwg::full_bind_event_handler(
            &ui.window.handle,
            handle_events,
        ));

        Ok(ui)
    }
}

impl Drop for LauncherAppUi {
    fn drop(&mut self) {
        if let Some(handler) = self.default_handler.borrow().as_ref() {
            nwg::unbind_event_handler(handler);
        }
    }
}

impl Deref for LauncherAppUi {
    type Target = LauncherApp;

    fn deref(&self) -> &LauncherApp {
        &self.inner
    }
}

pub fn run() {
    if let Err(err) = run_inner() {
        nwg::modal_error_message(
            &nwg::ControlHandle::NoHandle,
            "BeeBotOS Launcher",
            &format!("启动 Launcher 失败：{err}"),
        );
    }
}

fn run_inner() -> Result<(), String> {
    nwg::init().map_err(|err| err.to_string())?;
    nwg::Font::set_global_family("Segoe UI").map_err(|err| err.to_string())?;
    let app_dir = std::env::current_exe()
        .map_err(|err| err.to_string())?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "无法定位应用目录".to_owned())?;
    let _ui = LauncherApp::build_ui(LauncherApp {
        app_dir,
        ..Default::default()
    })
    .map_err(|err| err.to_string())?;
    nwg::dispatch_thread_events();
    Ok(())
}

fn build_label(
    window: &nwg::Window,
    label: &mut nwg::Label,
    text: &str,
    y: i32,
) -> Result<(), nwg::NwgError> {
    nwg::Label::builder()
        .text(text)
        .size((130, 28))
        .position((20, y))
        .parent(window)
        .build(label)
}

fn build_key_input(
    window: &nwg::Window,
    input: &mut nwg::TextInput,
    value: &str,
    y: i32,
    focus: bool,
) -> Result<(), nwg::NwgError> {
    nwg::TextInput::builder()
        .size((560, 28))
        .position((160, y))
        .text(value)
        .password(Some('*'))
        .focus(focus)
        .parent(window)
        .build(input)
}

fn build_button(
    window: &nwg::Window,
    button: &mut nwg::Button,
    text: &str,
    position: (i32, i32),
    size: (i32, i32),
) -> Result<(), nwg::NwgError> {
    nwg::Button::builder()
        .text(text)
        .position(position)
        .size(size)
        .parent(window)
        .build(button)
}

fn run_powershell_action(
    app_dir: &Path,
    action: &str,
    title: &str,
    secrets: &[String],
) -> TaskResult {
    let runner = app_dir.join("beebotos-run.ps1");
    if !runner.exists() {
        return TaskResult {
            title: title.to_owned(),
            body: format!(
                "未找到运行脚本：{}\n请确认 Launcher 位于 BeeBotOS 安装目录。",
                runner.display()
            ),
            success: false,
        };
    }

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&runner)
        .arg(action)
        .arg("all")
        .current_dir(app_dir)
        .output();

    match output {
        Ok(output) => {
            let success = output.status.success();
            let mut body = String::new();
            push_section(
                &mut body,
                "stdout",
                &String::from_utf8_lossy(&output.stdout),
            );
            push_section(
                &mut body,
                "stderr",
                &String::from_utf8_lossy(&output.stderr),
            );
            if !success {
                push_section(&mut body, "logs", &failure_log_hint(app_dir, secrets));
            }
            if body.trim().is_empty() {
                body.push_str("命令已执行，无输出。");
            }
            TaskResult {
                title: title.to_owned(),
                body: redact_secrets(body, secrets),
                success,
            }
        }
        Err(err) => TaskResult {
            title: title.to_owned(),
            body: format!("执行 powershell.exe 失败：{err}"),
            success: false,
        },
    }
}

fn open_web(app_dir: &Path) -> TaskResult {
    let result = Command::new("cmd")
        .args(["/C", "start", "", WEB_URL])
        .current_dir(app_dir)
        .status();
    match result {
        Ok(status) => TaskResult {
            title: "打开 Web".to_owned(),
            body: format!("已请求系统默认浏览器打开：{WEB_URL}"),
            success: status.success(),
        },
        Err(err) => TaskResult {
            title: "打开 Web".to_owned(),
            body: format!("打开浏览器失败：{err}\n可手动访问：{WEB_URL}"),
            success: false,
        },
    }
}

fn read_logs(app_dir: &Path, secrets: &[String]) -> TaskResult {
    let log_dir = app_dir.join("data").join("logs");
    let mut body = String::new();
    for name in [
        "gateway.err",
        "web.err",
        "beehub.err",
        "gateway.log",
        "web.log",
        "beehub.log",
    ] {
        let path = log_dir.join(name);
        if path.exists() {
            match read_tail(&path, 12_000) {
                Ok(contents) => push_section(&mut body, name, &contents),
                Err(err) => push_section(&mut body, name, &format!("读取失败：{err}")),
            }
        }
    }

    if body.trim().is_empty() {
        body = format!("未找到日志文件：{}", log_dir.display());
    }

    TaskResult {
        title: "查看日志".to_owned(),
        body: redact_secrets(body, secrets),
        success: true,
    }
}

fn failure_log_hint(app_dir: &Path, secrets: &[String]) -> String {
    let log_result = read_logs(app_dir, secrets);
    format!(
        "服务启动失败时优先查看 data/logs/*.err。\n\n{}",
        log_result.body
    )
}

fn read_tail(path: &Path, max_bytes: usize) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let start = bytes.len().saturating_sub(max_bytes);
    Ok(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

fn push_section(body: &mut String, title: &str, contents: &str) {
    let contents = contents.trim();
    if contents.is_empty() {
        return;
    }
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str("== ");
    body.push_str(title);
    body.push_str(" ==\n");
    body.push_str(contents);
}

fn redact_secrets(mut text: String, secrets: &[String]) -> String {
    for secret in secrets {
        let secret = secret.trim();
        if secret.len() >= 6 {
            text = text.replace(secret, "[redacted]");
        }
    }
    text
}
