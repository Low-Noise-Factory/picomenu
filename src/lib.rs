#![no_std]

use core::future::Future;
use core::marker::PhantomData;
use core::str;
use ufmt::uWrite;

#[derive(Debug)]
pub enum IoDeviceError {
    Disconnected,
}

#[derive(Debug)]
pub enum MenuError {
    UnkownCommand,
    Io(IoDeviceError),
    InputBufferOverflow,
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

impl<IO: IoDevice> Output<'_, IO> {
    pub async fn write(&mut self, s: &str) {
        self.io_device.write_packet(s.as_bytes()).await;
    }

    pub async fn flush_buffer(&mut self) {
        self.io_device
            .write_packet(&self.buffer[..self.buffer_idx])
            .await;
    }
}

#[derive(Debug)]
pub struct OutputBufferOverflow {}

impl<IO: IoDevice> uWrite for Output<'_, IO> {
    type Error = OutputBufferOverflow;

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        let bytes = s.as_bytes();

        let start_idx = self.buffer_idx;
        if start_idx >= self.buffer.len() {
            return Err(OutputBufferOverflow {});
        }

        let end_idx = start_idx + bytes.len();
        if end_idx >= self.buffer.len() {
            return Err(OutputBufferOverflow {});
        }

        self.buffer[start_idx..end_idx].clone_from_slice(bytes);
        self.buffer_idx = end_idx;
        Ok(())
    }
}

#[macro_export]
macro_rules! outwriteln {
    ($out:expr, $($tt:tt)*) => {{
        match ufmt::uwriteln!($out, $($tt)*) {
            Ok(_) => { $out.flush_buffer().await; Ok(()) },
            e => e,
        }
    }}
}

trait ExecuteOrForward<IO: IoDevice> {
    async fn execute_or_forward(&self, cmd: &str, output: &mut Output<IO>)
        -> Result<(), MenuError>;
}

pub trait Command<IO: IoDevice> {
    fn execute(&self, output: &mut Output<'_, IO>) -> impl Future<Output = ()>;
}

struct CommandHolder<IO: IoDevice, CMD: Command<IO>> {
    name: &'static str,
    cmd: CMD,
    _marker: PhantomData<IO>,
}

impl<IO: IoDevice, CMD: Command<IO>> CommandHolder<IO, CMD> {
    async fn try_execute(&self, cmd: &str, output: &mut Output<'_, IO>) -> Result<(), ()> {
        if cmd == self.name {
            self.cmd.execute(output).await;
            Ok(())
        } else {
            Err(())
        }
    }
}

impl<IO: IoDevice, CMD: Command<IO>> CommandHolder<IO, CMD> {
    pub fn new(name: &'static str, ce: CMD) -> Self {
        Self {
            name,
            cmd: ce,
            _marker: PhantomData,
        }
    }
}

pub struct NullRouter {}

impl<IO: IoDevice> ExecuteOrForward<IO> for NullRouter {
    async fn execute_or_forward(
        &self,
        _cmd: &str,
        _output: &mut Output<'_, IO>,
    ) -> Result<(), MenuError> {
        Err(MenuError::UnkownCommand)
    }
}

struct Router<IO: IoDevice, NextRouter: ExecuteOrForward<IO>, CMD: Command<IO>> {
    cmd: CommandHolder<IO, CMD>,
    next_router: NextRouter,
}

impl<IO: IoDevice, NextRouter: ExecuteOrForward<IO>, CMD: Command<IO>> ExecuteOrForward<IO>
    for Router<IO, NextRouter, CMD>
{
    async fn execute_or_forward(
        &self,
        cmd: &str,
        output: &mut Output<'_, IO>,
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

pub trait Menu<IO: IoDevice> {
    fn read_input(&mut self) -> impl Future<Output = Result<(), MenuError>>;
    fn add_command<CMD: Command<IO>>(self, name: &'static str, cmd: CMD) -> impl Menu<IO>;
}

impl<IO: IoDevice, HeadRouter: ExecuteOrForward<IO>> Menu<IO> for MenuImpl<'_, IO, HeadRouter> {
    async fn read_input(&mut self) -> Result<(), MenuError> {
        let mut input_buffer = [0; BUF_SIZE];
        let n = self.output.io_device.read_packet(&mut input_buffer).await?;
        let data = &input_buffer[..n];

        for char in data {
            self.input_byte(*char).await?;
        }

        Ok(())
    }

    fn add_command<CMD: Command<IO>>(self, name: &'static str, cmd: CMD) -> impl Menu<IO> {
        let new_router = Router {
            cmd: CommandHolder::new(name, cmd),
            next_router: self.head_router,
        };

        MenuImpl {
            head_router: new_router,
            input_buffer: self.input_buffer,
            input_buffer_idx: self.input_buffer_idx,
            output: self.output,
        }
    }
}

struct MenuImpl<'d, IO: IoDevice, HeadRouter: ExecuteOrForward<IO>> {
    head_router: HeadRouter,
    input_buffer: [u8; BUF_SIZE],
    input_buffer_idx: usize,
    output: Output<'d, IO>,
}

impl<'d, T: IoDevice, HeadRouter: ExecuteOrForward<T>> MenuImpl<'d, T, HeadRouter> {
    async fn input_byte(&mut self, char: u8) -> Result<(), MenuError> {
        if self.input_buffer_idx >= self.input_buffer.len() {
            return Err(MenuError::InputBufferOverflow);
        }

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

pub fn new_menu<IO: IoDevice>(io_device: &mut IO) -> impl Menu<IO> + use<'_, IO> {
    MenuImpl {
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

pub async fn run_menu<IO: IoDevice>(mut menu: impl Menu<IO>) {
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
                    MenuError::InputBufferOverflow => {
                        // FIXME: print a message instead
                        panic!("Input buffer overflow");
                    }
                }
            }
            _ => continue,
        }
    }
}
