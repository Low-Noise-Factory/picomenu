#![no_std]

use core::future::Future;
use core::marker::PhantomData;
use core::str;

use ufmt::uWrite;

pub enum IoDeviceError {
    Disconnected,
}

pub enum MenuError {
    UnkownCommand,
    Io(IoDeviceError),
}

impl From<IoDeviceError> for MenuError {
    fn from(value: IoDeviceError) -> Self {
        MenuError::Io(value)
    }
}

pub trait IoDevice {
    fn write_packet(&mut self, data: &[u8]) -> impl Future<Output = ()>;
    fn read_packet(
        &mut self,
        data: &mut [u8],
    ) -> impl Future<Output = Result<usize, IoDeviceError>>;
}

pub struct Output<'d, T: IoDevice> {
    io_device: &'d mut T,
    buffer: [u8; BUF_SIZE],
    buffer_idx: usize,
}

impl<'d, T: IoDevice> Output<'d, T> {
    pub async fn write(&mut self, s: &str) {
        self.io_device.write_packet(s.as_bytes()).await;
    }

    pub async fn flush_buffer(&mut self) {
        self.io_device
            .write_packet(&self.buffer[..self.buffer_idx])
            .await;
    }
}

impl<'d, T: IoDevice> uWrite for Output<'d, T> {
    type Error = ();

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        // FIXME: handle overflow

        let bytes = s.as_bytes();
        let start_idx = self.buffer_idx;
        let end_idx = start_idx + bytes.len();
        self.buffer[start_idx..end_idx].clone_from_slice(bytes);
        self.buffer_idx = end_idx;
        Ok(())
    }
}

#[macro_export]
macro_rules! outwriteln {
    ($out:expr, $($tt:tt)*) => {{
        ufmt::uwriteln!($out, $($tt)*).unwrap();
        $out.flush_buffer().await;
    }}
}

pub trait ExecuteOrForward<T: IoDevice> {
    fn execute_or_forward(
        &self,
        cmd: &str,
        output: &mut Output<T>,
    ) -> impl Future<Output = Result<(), MenuError>>;
}

pub trait Execute<T: IoDevice> {
    fn exe(&self, output: &mut Output<'_, T>) -> impl Future<Output = ()>;
}

pub struct Command<T: IoDevice, CE: Execute<T>> {
    name: &'static str,
    ce: CE,
    _marker: PhantomData<T>,
}

impl<T: IoDevice, CE: Execute<T>> Command<T, CE> {
    async fn try_execute(&self, cmd: &str, output: &mut Output<'_, T>) -> Result<(), ()> {
        if cmd == self.name {
            self.ce.exe(output).await;
            Ok(())
        } else {
            Err(())
        }
    }
}

impl<T: IoDevice, CE: Execute<T>> Command<T, CE> {
    pub fn new(name: &'static str, ce: CE) -> Self {
        Self {
            name,
            ce,
            _marker: PhantomData,
        }
    }
}

pub struct NullRouter {}

impl<T: IoDevice> ExecuteOrForward<T> for NullRouter {
    async fn execute_or_forward(
        &self,
        _cmd: &str,
        _output: &mut Output<'_, T>,
    ) -> Result<(), MenuError> {
        Err(MenuError::UnkownCommand)
    }
}

pub struct Router<T: IoDevice, NextRouter: ExecuteOrForward<T>, CE: Execute<T>> {
    cmd: Command<T, CE>,
    next_router: NextRouter,
}

impl<T: IoDevice, NextRouter: ExecuteOrForward<T>, CE: Execute<T>> ExecuteOrForward<T>
    for Router<T, NextRouter, CE>
{
    async fn execute_or_forward(
        &self,
        cmd: &str,
        output: &mut Output<'_, T>,
    ) -> Result<(), MenuError> {
        if self.cmd.try_execute(cmd, output).await.is_ok() {
            Ok(())
        } else {
            self.next_router.execute_or_forward(cmd, output).await
        }
    }
}

// TODO: make this a parameter
const BUF_SIZE: usize = 128;

pub struct Menu<'d, T: IoDevice, HeadRouter: ExecuteOrForward<T>> {
    head_router: HeadRouter,
    input_buffer: [u8; BUF_SIZE],
    input_buffer_idx: usize,
    output: Output<'d, T>,
}

impl<'d, T: IoDevice, HeadRouter: ExecuteOrForward<T>> Menu<'d, T, HeadRouter> {
    pub fn add_command<CE: Execute<T>>(
        self,
        cmd: Command<T, CE>,
    ) -> Menu<'d, T, Router<T, HeadRouter, CE>> {
        let new_router = Router {
            cmd,
            next_router: self.head_router,
        };
        Menu {
            head_router: new_router,
            input_buffer: self.input_buffer,
            input_buffer_idx: self.input_buffer_idx,
            output: self.output,
        }
    }

    async fn read_input(&mut self) -> Result<(), MenuError> {
        let mut input_buffer = [0; BUF_SIZE];
        let n = self.output.io_device.read_packet(&mut input_buffer).await?;
        let data = &input_buffer[..n];

        for char in data {
            self.input_byte(*char).await?;
        }

        Ok(())
    }

    async fn input_byte(&mut self, char: u8) -> Result<(), MenuError> {
        // FIXME: handle buffer overflow

        if char == b'\n' {
            self.process_buffer().await?;
        } else {
            self.input_buffer[self.input_buffer_idx] = char;
            self.input_buffer_idx += 1;
        }

        Ok(())
    }

    async fn process_buffer(&mut self) -> Result<(), MenuError> {
        let cmd = str::from_utf8(&self.input_buffer[..self.input_buffer_idx]).unwrap();
        self.head_router
            .execute_or_forward(cmd, &mut self.output)
            .await?;
        self.input_buffer_idx = 0;
        Ok(())
    }
}

pub fn new_menu<T: IoDevice>(io_device: &mut T) -> Menu<'_, T, NullRouter> {
    Menu {
        head_router: NullRouter {},
        input_buffer: [0; BUF_SIZE],
        input_buffer_idx: 0,
        output: Output {
            io_device,
            buffer: [0; BUF_SIZE],
            buffer_idx: 0,
        },
    }
}

pub async fn run_menu<T: IoDevice, H: ExecuteOrForward<T>>(mut menu: Menu<'_, T, H>) {
    loop {
        match menu.read_input().await {
            Err(e) => {
                match e {
                    MenuError::Io(IoDeviceError::Disconnected) => {
                        return;
                    }
                    MenuError::UnkownCommand => {
                        // FIXME: print a message instead
                        panic!("Unkown command");
                    }
                }
            }
            _ => continue,
        }
    }
}
