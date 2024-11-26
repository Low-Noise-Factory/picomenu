#![no_std]

use core::future::Future;
use core::marker::PhantomData;
use core::str::{self, Utf8Error};
use ufmt::uWrite;

#[derive(Debug)]
pub enum IoDeviceError {
    Disconnected,
    InputBufferOverflow,
}

enum MenuError {
    UnkownCommand,
    Io(IoDeviceError),
    Utf8,
}

impl From<IoDeviceError> for MenuError {
    fn from(value: IoDeviceError) -> Self {
        MenuError::Io(value)
    }
}

impl From<Utf8Error> for MenuError {
    fn from(_: Utf8Error) -> Self {
        MenuError::Utf8
    }
}

pub trait IoDevice {
    fn write_packet(&mut self, data: &[u8]) -> impl Future<Output = ()>;
    fn read_packet(
        &mut self,
        data: &mut [u8],
    ) -> impl Future<Output = Result<usize, IoDeviceError>>;
}

pub struct Output<'d, IO: IoDevice> {
    io_device: &'d mut IO,
    buffer: &'d mut [u8],
    buffer_idx: &'d mut usize,
}

impl<IO: IoDevice> Output<'_, IO> {
    pub async fn write(&mut self, s: &str) {
        self.io_device.write_packet(s.as_bytes()).await;
    }

    pub async fn flush_buffer(&mut self) {
        self.io_device
            .write_packet(&self.buffer[..*self.buffer_idx])
            .await;

        *self.buffer_idx = 0;
    }
}

#[derive(Debug)]
pub struct OutputBufferOverflow {}

impl<IO: IoDevice> uWrite for Output<'_, IO> {
    type Error = OutputBufferOverflow;

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        let bytes = s.as_bytes();

        let start_idx = *self.buffer_idx;
        if start_idx >= self.buffer.len() {
            return Err(OutputBufferOverflow {});
        }

        let end_idx = start_idx + bytes.len();
        if end_idx >= self.buffer.len() {
            return Err(OutputBufferOverflow {});
        }

        self.buffer[start_idx..end_idx].clone_from_slice(bytes);
        *self.buffer_idx = end_idx;
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

trait ExecuteOrForward<IO: IoDevice, S> {
    async fn execute_or_forward(
        &self,
        cmd: &str,
        output: &mut Output<IO>,
        state: &mut S,
    ) -> Result<(), MenuError>;
}

pub trait Command<IO: IoDevice, S> {
    fn execute(output: &mut Output<'_, IO>, state: &mut S) -> impl Future<Output = ()>;
}

struct CommandHolder<IO: IoDevice, S, CMD: Command<IO, S>> {
    name: &'static str,
    _cmd_marker: PhantomData<CMD>,
    _io_marker: PhantomData<IO>,
    _state_marker: PhantomData<S>,
}

impl<IO: IoDevice, S, CMD: Command<IO, S>> CommandHolder<IO, S, CMD> {
    async fn try_execute(
        &self,
        cmd: &str,
        output: &mut Output<'_, IO>,
        state: &mut S,
    ) -> Result<(), ()> {
        if cmd == self.name {
            CMD::execute(output, state).await;
            Ok(())
        } else {
            Err(())
        }
    }
}

impl<IO: IoDevice, S, CMD: Command<IO, S>> CommandHolder<IO, S, CMD> {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            _cmd_marker: PhantomData,
            _io_marker: PhantomData,
            _state_marker: PhantomData,
        }
    }
}

pub struct NullRouter {}

impl<IO: IoDevice, S> ExecuteOrForward<IO, S> for NullRouter {
    async fn execute_or_forward(
        &self,
        _cmd: &str,
        _output: &mut Output<'_, IO>,
        _state: &mut S,
    ) -> Result<(), MenuError> {
        Err(MenuError::UnkownCommand)
    }
}

struct Router<IO: IoDevice, S, NextRouter: ExecuteOrForward<IO, S>, CMD: Command<IO, S>> {
    cmd: CommandHolder<IO, S, CMD>,
    next_router: NextRouter,
}

impl<IO: IoDevice, S, NextRouter: ExecuteOrForward<IO, S>, CMD: Command<IO, S>>
    ExecuteOrForward<IO, S> for Router<IO, S, NextRouter, CMD>
{
    async fn execute_or_forward(
        &self,
        cmd: &str,
        output: &mut Output<'_, IO>,
        state: &mut S,
    ) -> Result<(), MenuError> {
        if self.cmd.try_execute(cmd, output, state).await.is_ok() {
            Ok(())
        } else {
            self.next_router
                .execute_or_forward(cmd, output, state)
                .await
        }
    }
}

pub trait Menu<IO: IoDevice, S> {
    fn add_command<CMD: Command<IO, S>>(self, name: &'static str) -> impl Menu<IO, S>;
    fn can_run(&mut self) -> impl Future<Output = bool>;
    fn borrow_state(&self) -> &S;
    fn borrow_state_mut(&mut self) -> &mut S;
}

impl<IO: IoDevice, S, HeadRouter: ExecuteOrForward<IO, S>> Menu<IO, S>
    for MenuImpl<'_, IO, S, HeadRouter>
{
    fn add_command<CMD: Command<IO, S>>(self, name: &'static str) -> impl Menu<IO, S> {
        let new_router = Router {
            cmd: CommandHolder::<IO, S, CMD>::new(name),
            next_router: self.head_router,
        };

        MenuImpl {
            head_router: new_router,
            input_buffer: self.input_buffer,
            input_buffer_idx: self.input_buffer_idx,
            output_buffer: self.output_buffer,
            output_buffer_idx: self.output_buffer_idx,
            io_device: self.io_device,
            state: self.state,
        }
    }

    async fn can_run(&mut self) -> bool {
        if let Err(e) = self.read_input().await {
            match e {
                MenuError::Io(IoDeviceError::Disconnected) => {
                    return false;
                }
                MenuError::UnkownCommand => {
                    self.println("Unknown command").await;
                }
                MenuError::Io(IoDeviceError::InputBufferOverflow) => {
                    self.println("Input buffer overflow").await;
                }
                MenuError::Utf8 => {
                    self.println("Input UTF8 error").await;
                }
            }
        }

        true
    }

    fn borrow_state(&self) -> &S {
        &self.state
    }

    fn borrow_state_mut(&mut self) -> &mut S {
        &mut self.state
    }
}

struct MenuImpl<'d, IO: IoDevice, S, HeadRouter: ExecuteOrForward<IO, S>> {
    head_router: HeadRouter,
    input_buffer: &'d mut [u8],
    input_buffer_idx: usize,
    output_buffer: &'d mut [u8],
    output_buffer_idx: usize,
    io_device: &'d mut IO,
    state: S,
}

impl<'d, IO: IoDevice, S, HeadRouter: ExecuteOrForward<IO, S>> MenuImpl<'d, IO, S, HeadRouter> {
    async fn read_input(&mut self) -> Result<(), MenuError> {
        if self.input_buffer_idx >= self.input_buffer.len() {
            return Err(IoDeviceError::InputBufferOverflow.into());
        }

        let n = {
            let buf = &mut self.input_buffer[self.input_buffer_idx..];
            self.io_device.read_packet(buf).await?
        };

        let start_idx = self.input_buffer_idx;
        let end_idx = self.input_buffer_idx + n;

        for i in start_idx..end_idx {
            let char = self.input_buffer[i];
            self.input_buffer_idx = i;

            if char == b'\n' {
                self.process_buffer().await?;
            }
        }

        Ok(())
    }

    async fn process_buffer(&mut self) -> Result<(), MenuError> {
        let cmd_string = str::from_utf8(&self.input_buffer[..self.input_buffer_idx])?;

        let mut output = Output {
            io_device: self.io_device,
            buffer: self.output_buffer,
            buffer_idx: &mut self.output_buffer_idx,
        };

        self.head_router
            .execute_or_forward(cmd_string, &mut output, &mut self.state)
            .await?;
        self.input_buffer_idx = 0;
        Ok(())
    }

    async fn println(&mut self, msg: &'static str) {
        let mut output = Output {
            io_device: self.io_device,
            buffer: self.output_buffer,
            buffer_idx: &mut self.output_buffer_idx,
        };

        outwriteln!(output, "{}", msg).unwrap();
    }
}

pub fn new_menu<'d, IO: IoDevice, S>(
    io_device: &'d mut IO,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
    state: S,
) -> impl Menu<IO, S> + use<'d, IO, S> {
    MenuImpl {
        head_router: NullRouter {},
        input_buffer,
        input_buffer_idx: 0,
        output_buffer,
        output_buffer_idx: 0,
        io_device,
        state,
    }
}

pub async fn run_menu<IO: IoDevice, S>(mut menu: impl Menu<IO, S>) -> impl Menu<IO, S> {
    while menu.can_run().await {}
    menu
}
