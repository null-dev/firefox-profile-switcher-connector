use crossbeam_channel::unbounded as unbounded_channel;
use directories::UserDirs;
use rfd::FileDialog;
use std::path::PathBuf;
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};

pub struct Windowing {
    event_loop: EventLoop<UserEvent>,
}

#[derive(Clone, Debug)]
pub struct WindowingHandle {
    event_loop_proxy: EventLoopProxy<UserEvent>,
}

struct UserEvent {
    task: Box<dyn FnOnce() + Send>,
}

impl Windowing {
    pub fn new() -> Self {
        Self {
            event_loop: EventLoop::with_user_event(),
        }
    }

    pub fn run_event_loop(self) {
        self.event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            if let winit::event::Event::UserEvent(evt) = event {
                let task = evt.task;
                task();
            }
        });
    }

    pub fn get_handle(&self) -> WindowingHandle {
        WindowingHandle {
            event_loop_proxy: self.event_loop.create_proxy(),
        }
    }
}

impl WindowingHandle {
    pub fn open_avatar_picker(&self) -> Option<Vec<PathBuf>> {
        let user_dirs = UserDirs::new();

        self.exec_on_main_thread(move || {
            let home = user_dirs.as_ref().map(|d| d.home_dir());

            let mut file_dialog = FileDialog::new()
                .set_title("Select pictures to add")
                .add_filter("PNG Image", &["png"])
                .add_filter("JPEG Image", &["jpg", "jpeg"]);

            if let Some(home) = home {
                file_dialog = file_dialog.set_directory(home)
            }

            file_dialog.pick_files()
        })
    }

    fn exec_on_main_thread<T: FnOnce() -> Z, Z>(&self, task: T) -> Z
    where
        T: Send + 'static,
        Z: Send + 'static,
    {
        let (sender, receiver) = unbounded_channel::<Z>();
        if self
            .event_loop_proxy
            .send_event(UserEvent {
                task: Box::new(move || {
                    let result = task();

                    sender
                        .send(result)
                        .expect("failed to send message from main thread");
                }),
            })
            .is_err()
        {
            panic!("main thread died?");
        };

        receiver
            .recv()
            .expect("failed to receive result from main thread")
    }
}
