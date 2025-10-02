use iced::widget;
use iced::{Task, Theme};
use std::fs::{File, remove_file};
use std::io::{ Write, Seek, SeekFrom};
use iced::Subscription;  // جدید: برای Subscription
use iced_futures::futures::StreamExt;  // جدید: برای map روی stream
use rand::Rng;
use flume::{Sender, Receiver};

struct App {
    file: String,
    progress: f32,
    erasing: bool,
    receiver: Option<Receiver<Progress>>,
}

#[derive(Clone, Debug)]
enum Progress {
    Updated(f32),
    Finished(bool),
}

#[derive(Debug, Clone)]
enum Message {
    SelectFile,
    FileOpened(Result<String, String>),
    EraseFile,
    Progress(Progress),
}

impl App {
    fn securely_overwrite(path: &str, passes: usize, tx: &Sender<Progress>) -> std::io::Result<()> {
        let mut file = File::options()
            .read(true)
            .write(true)
            .open(path)?;

        let file_size = file.metadata()?.len() as usize;
        if file_size == 0 {
            remove_file(path)?;
            tx.send(Progress::Updated(100.0)).map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Channel error"))?;
            return Ok(());
        }

        let mut rng = rand::thread_rng();
        let buffer_size = 4096;
        let mut buffer = vec![0u8; buffer_size];

        let total_work = passes as u64 * file_size as u64;
        let mut completed_work: u64 = 0;
        let mut chunk_count = 0;

        for _pass in 0..passes {
            let mut remaining = file_size;
            file.seek(SeekFrom::Start(0))?;

            while remaining > 0 {
                let current_chunk = buffer_size.min(remaining);
                for i in 0..current_chunk {
                    buffer[i] = rng.r#gen::<u8>();  
                }
                file.write_all(&buffer[..current_chunk])?;
                remaining -= current_chunk;
                completed_work += current_chunk as u64;
                chunk_count += 1;

                // محدود کردن send: هر 100 chunk (برای فایل 200MB حدود 500 send)
                if chunk_count % 100 == 0 {
                    let progress = (completed_work as f32 / total_work as f32) * 100.0;
                    tx.send(Progress::Updated(progress)).map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Channel error"))?;
                }
            }
            file.sync_all()?;
        }

        drop(file);
        remove_file(path)?;
        tx.send(Progress::Updated(100.0)).map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Channel error"))?;
        Ok(())
    }

    fn new() -> Self {
        Self {
            file: "".to_string(),
            progress: 0.0,
            erasing: false,
            receiver: None,
        }
    }

    fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::EraseFile => {
                println!("Erasing file start");
                if !self.erasing {
                    let (tx, rx) = flume::bounded(1000);  // ظرفیت بزرگ برای فایل‌های بزرگ
                    self.receiver = Some(rx);
                    self.erasing = true;
                    self.progress = 0.0;

                    let path = self.file.clone();
                    std::thread::spawn(move || {
                        let result = Self::securely_overwrite(&path, 3, &tx).is_ok();
                        tx.send(Progress::Finished(result)).expect("Channel error in thread");
                    });
                }
                iced::Task::none()
            },
            Message::Progress(p) => {
                println!("Progress received: {:?}", p);
                match p {
                    Progress::Updated(val) => {
                        self.progress = val;
                    }
                    Progress::Finished(success) => {
                        println!("Erasing file finished");
                        self.erasing = false;
                        self.receiver = None;
                        if !success {
                            eprintln!("Error during file erasure");
                        }
                        self.progress = 100.0;
                    }
                }
                iced::Task::none()
            },
            Message::SelectFile => Task::perform(open_file(&["*"]), Message::FileOpened),
            Message::FileOpened(result) => {
                match result {
                    Ok(file_path) => {
                        self.file = file_path;
                    }
                    Err(e) => {
                        eprintln!("Error selecting file: {}", e);
                    }
                }
                iced::Task::none()
            }
        }
    }

    fn view(&self) -> iced::Element<'_, Message> {
        let row = widget::container(
            widget::row![
                widget::button("Open file").on_press(Message::SelectFile),
                widget::container(widget::text!(" File: {}", self.file)).padding(7),
            ]
                .width(iced::Length::Fill)
                .height(50)
                .spacing(10),
        )
            .center_x(iced::Length::Fill);

        let erase_button = if self.erasing {
            widget::button("Erasing...")
        } else {
            widget::button("Erase file").on_press(Message::EraseFile)
        };

        widget::container(widget::column![
            row,
            widget::row![
                widget::progress_bar(0.0..=100.0, self.progress),
                erase_button,
            ].spacing(10)
        ])
            .padding(10)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        if let Some(receiver) = self.receiver.clone() {
            Subscription::run_with_id(
                "erase_subscription",
                Box::pin(receiver.into_stream().map(Message::Progress)),
            )
        } else {
            Subscription::none()
        }
    }
}

async fn open_file(support_ext: &[impl ToString]) -> Result<String, String> {
    println!("Opening file..., {}", support_ext.len());
    let picked_file = rfd::AsyncFileDialog::new()
        .set_title("Open file...")
        .add_filter("All files", &["*"])
        .pick_file()
        .await;

    let picked_file = match picked_file {
        Some(file) => file,
        None => return Err("No file was selected.".to_string()),
    };

    let path = match picked_file.path().to_str() {
        Some(path) => path,
        None => return Err("File path is not valid UTF-8.".to_string()),
    };

    Ok(path.to_string())
}

fn theme(_state: &App) -> Theme {
    Theme::Nord
}

fn main() -> Result<(), iced::Error> {
    iced::application("File Eraser", App::update, App::view)
        .subscription(App::subscription)  // اضافه کردن subscription به application
        .theme(theme)
        .window_size(iced::Size::new(750.0, 100.0))
        .position(iced::window::Position::Centered)
        .run_with(|| (App::new(), iced::Task::none()))
}