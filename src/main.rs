use futures::channel::mpsc;
use futures::{SinkExt, StreamExt, Stream};

use std::os::windows::process::CommandExt;
use tokio::time::Duration;
use tokio::process::{Command};
use tokio::io::{BufReader, AsyncBufReadExt};
use std::process::Stdio;

use glow::HasContext;
use imgui_winit_support::WinitPlatform;
use std::iter::FromIterator;

pub type Window = glutin::WindowedContext<glutin::PossiblyCurrent>;

fn glow_context(window: &Window) -> glow::Context {
    unsafe { glow::Context::from_loader_function(|s| window.get_proc_address(s).cast()) }
}

const LOG_SIZE:usize = 50000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        panic!("not enough arguments");
    }
    let (mut tx , mut rx) = mpsc::channel::<String>(1000);
    let APP_NAME = "LogKX";

    tokio::spawn(async move {
        let mut child = Command::new(args[1].to_owned())
            .current_dir(args[2].to_owned())
            .args(&args[3..args.len()].to_owned())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .creation_flags(0x08000000)
            .spawn().expect("failed to start child process");

        let sleep = Duration::from_millis(16);
        loop {
            if let Some(stdout) = child.stdout.take() {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(line) = lines.next_line().await.ok().and_then(|o| o) {
                    tx.send(line).await;
                }
            }
            if let Some(stderr) = child.stderr.take() {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(line) = lines.next_line().await.ok().and_then(|o| o) {
                    tx.send(line).await;
                }
            }
            tokio::time::sleep(sleep).await;
        }
    });

    //
    // RENDERING
    //

    let event_loop = glutin::event_loop::EventLoop::new();
    let window = glutin::window::WindowBuilder::new()
        .with_title("LogKX")
        .with_inner_size(glutin::dpi::LogicalSize::new(640, 320))
        .with_min_inner_size(glutin::dpi::LogicalSize::new(360, 200));
    let window = glutin::ContextBuilder::new()
        .build_windowed(window, &event_loop)
        .expect("could not create window");
    let window = unsafe {
        window
            .make_current()
            .expect("could not make window context current")
    };

    let mut imgui_context = imgui::Context::create();
    imgui_context.fonts().add_font(&[imgui::FontSource::DefaultFontData { config: None }]);
    imgui_context.set_ini_filename(None);
    imgui_context.set_log_filename(None);
    imgui_context.style_mut().child_rounding = 0.0;
    imgui_context.style_mut().frame_rounding = 0.0;
    imgui_context.style_mut().grab_rounding = 0.0;
    imgui_context.style_mut().popup_rounding = 0.0;
    imgui_context.style_mut().scrollbar_rounding = 0.0;
    imgui_context.style_mut().window_rounding = 0.0;
    imgui_context.style_mut().window_title_align = [0.5, 0.5];
    imgui_context.style_mut().window_border_size = 0.0;
    imgui_context.style_mut().child_border_size = 0.0;
    imgui_context.style_mut().frame_border_size = 0.0;
    imgui_context.style_mut().tab_border_size = 0.0;

    let mut winit_platform = WinitPlatform::init(&mut imgui_context);
    winit_platform.attach_window(
        imgui_context.io_mut(),
        window.window(),
        imgui_winit_support::HiDpiMode::Rounded,
    );

    imgui_context.io_mut().font_global_scale = (1.0 / winit_platform.hidpi_factor()) as f32;

    // OpenGL context from glow
    let gl = glow_context(&window);

    // OpenGL renderer from this crate
    let mut ig_renderer = imgui_glow_renderer::AutoRenderer::initialize(gl, &mut imgui_context)
        .expect("failed to create renderer");

    let mut last_frame = std::time::Instant::now();

    let mut search = String::from("");
    let mut autoscroll = true;
    let mut log = Vec::new();

    event_loop.run(move |event, _, control_flow| {
        match event {
            glutin::event::Event::NewEvents(_) => {
                let now = std::time::Instant::now();
                imgui_context
                    .io_mut()
                    .update_delta_time(now.duration_since(last_frame));
                last_frame = now;
            }
            glutin::event::Event::MainEventsCleared => {
                winit_platform
                    .prepare_frame(imgui_context.io_mut(), window.window())
                    .unwrap();
                window.window().request_redraw();
            }
            glutin::event::Event::RedrawRequested(_) => {
                futures::executor::block_on(async {
                    loop {
                        let mut l = rx.try_next();
                        let linematch = match l {
                            Ok(val) => val,
                            Err(_) => None
                        };
                        if linematch.is_none() { break; }
                        log.push(linematch.unwrap());
                        l = rx.try_next();
                    }
                });

                // The renderer assumes you'll be clearing the buffer yourself
                unsafe { ig_renderer.gl_context().clear(glow::COLOR_BUFFER_BIT) };

                let wheelscroll = imgui_context.io().mouse_wheel;
                let ui = imgui_context.frame();
                imgui::Window::new(&ui, "Hello world")
                    .resizable(false)
                    .title_bar(false)
                    .movable(false)
                    .collapsible(false)
                    .position([0.0, 0.0], imgui::Condition::Always)
                    .size([window.window().inner_size().width as f32, window.window().inner_size().height as f32], imgui::Condition::Always)
                    .scrollable(true)
                    .build(|| {
                        if autoscroll {
                            ui.set_scroll_y(ui.scroll_max_y());
                        }
                        let group = ui.begin_group();
                        ui.input_text("",  &mut search).build();
                        ui.same_line();
                        if ui.button("Clear logs") {
                            log.clear();
                        }
                        group.end();
                        ui.dummy([0.0, 0.0]);
                        ui.separator();
                        for logline in log.iter() {
                            if !search.is_empty() && !logline.to_lowercase().contains(&search.to_lowercase()) {
                                continue;
                            }
                            ui.text_wrapped(logline);
                        }
                        if ui.is_mouse_down(imgui::MouseButton::Left) || wheelscroll != 0.0 {
                            if ui.scroll_y() != ui.scroll_max_y() {
                                if autoscroll == true {
                                    autoscroll = false;
                                    ui.set_scroll_y(ui.scroll_y() + wheelscroll);
                                }
                            } else {
                                autoscroll = true;
                            }
                        }
                    });

                winit_platform.prepare_render(ui, window.window());
                let draw_data = imgui_context.render();

                // This is the only extra render step to add
                ig_renderer
                    .render(draw_data)
                    .expect("error rendering imgui");

                window.swap_buffers().unwrap();
            }
            glutin::event::Event::WindowEvent {
                event: glutin::event::WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = glutin::event_loop::ControlFlow::Exit;
            }
            event => {
                winit_platform.handle_event(imgui_context.io_mut(), window.window(), &event);
            }
        }
    });
    Ok(())
}