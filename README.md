# Picomenu

Picomenu is a very simple library to implent command line menus on no-std async embedded systems such as those powered by Embassy. It is only meant for very simple menus and therefore does not support more advanced features at the moment.

Following is an an example of how to use the library:

```
struct VersionCommand {}
impl<IO: IoDevice> Command<IO, State> for VersionCommand {
    async fn execute(_args: Option<&str>, output: &mut Output<'_, IO>, state: &mut State) {
        outwriteln!(output, "Version: {}", state.version).unwrap();
    }

    fn help_string() -> &'static str {
        "Shows version"
    }
}

struct HelloCommand {}
impl<IO: IoDevice> Command<IO, State> for HelloCommand {
    async fn execute(args: Option<&str>, output: &mut Output<'_, IO>, _state: &mut State) {
        if let Some(name) = args {
            outwriteln!(output, "Hello {}!", name).unwrap();
        } else {
            outwriteln!(output, "Please enter your name").unwrap();
        }
    }

    fn help_string() -> &'static str {
        "Says hello"
    }
}

struct State {
    version: u32,
}

fn build_menu<'d>(
    device: &'d mut MockIo,
    input_buffer: &'d mut [u8],
    output_buffer: &'d mut [u8],
) -> impl Menu<MockIo, State> + use<'d> {
    let state = State {
        version: 2,
        overflowed: false,
    };

    new_menu(device, input_buffer, output_buffer, state)
        .add_command::<TestCommand>("test")
        .add_command::<VersionCommand>("version")
        .add_command::<HelloCommand>("hello")
}
```

Here is should be noted that the help command is also provided automatically. For more details on how to use the library, please have a look at `tests/menu.rs`.

The current feature set is sufficient for our needs so therefore we likely won't have time to address any feature requests :) But please feel free to contribute any features you may need yourself!