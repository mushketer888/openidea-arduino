//! OpenIDEA Arduino Plugin: Build, Upload and Serial Monitor commands
//! with interactive board and serial port pickers via Arduino CLI.

use openidea_plugin::host::{self, storage};
use openidea_plugin::{Error, Invocation, NotificationLevel, PickItem, Plugin, register_plugin};
use serde::Deserialize;

#[derive(Default)]
pub struct ArduinoPlugin;

#[derive(Clone, Deserialize)]
struct DetectedPortBoard {
    name: String,
}

#[derive(Clone, Default, Deserialize)]
struct DetectedPortDetails {
    #[serde(default)]
    address: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    boards: Vec<DetectedPortBoard>,
}

#[derive(Clone, Deserialize)]
struct DetectedPortEntry {
    #[serde(default)]
    port: Option<DetectedPortDetails>,
    #[serde(default)]
    address: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    boards: Vec<DetectedPortBoard>,
}

impl DetectedPortEntry {
    fn into_details(self) -> DetectedPortDetails {
        if let Some(port) = self.port {
            port
        } else {
            DetectedPortDetails {
                address: self.address,
                label: self.label,
                protocol: self.protocol,
                boards: self.boards,
            }
        }
    }
}

#[derive(Deserialize)]
struct DetectedPortsOutput {
    #[serde(default)]
    detected_ports: Vec<DetectedPortEntry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BoardListJson {
    Wrapped(DetectedPortsOutput),
    Legacy(Vec<DetectedPortEntry>),
}

fn sanitize_port(port: &str) -> String {
    let trimmed = port.trim();
    if trimmed.to_ascii_uppercase().starts_with("COM") {
        trimmed.replace(' ', "").to_ascii_uppercase()
    } else {
        trimmed.to_owned()
    }
}

impl Plugin for ArduinoPlugin {
    fn run_command(
        &mut self,
        id: &str,
        ctx: &Invocation,
        _arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        match id {
            "select-board" => self.select_board(),
            "select-port" => self.select_port(),
            "build" => self.build_sketch(ctx),
            "upload" => self.upload_sketch(ctx),
            "serial-monitor" => self.serial_monitor(ctx),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

impl ArduinoPlugin {
    fn current_board(&self) -> Option<String> {
        storage::get("board")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
    }

    fn current_port(&self) -> Option<String> {
        storage::get("port")
            .ok()
            .flatten()
            .map(|s| sanitize_port(&s))
            .filter(|s| !s.is_empty())
    }

    fn select_board(&self) -> Result<Option<String>, Error> {
        let mut items = Vec::new();

        // 1. Try querying arduino-cli board listall --format json
        if let Ok(output) = host::run_process(
            "arduino-cli",
            &[
                "board".into(),
                "listall".into(),
                "--format".into(),
                "json".into(),
            ],
            None,
        ) && output.status == Some(0)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&output.stdout)
            && let Some(boards) = val.get("boards").and_then(|b| b.as_array())
        {
            for b in boards {
                if let (Some(name), Some(fqbn)) = (
                    b.get("name").and_then(|n| n.as_str()),
                    b.get("fqbn").and_then(|f| f.as_str()),
                ) {
                    items.push(PickItem::new(fqbn, name).description(fqbn));
                }
            }
        }

        // 2. Fall back to standard popular boards if listall returned nothing
        if items.is_empty() {
            let defaults = [
                ("arduino:avr:uno", "Arduino Uno"),
                ("arduino:avr:nano", "Arduino Nano"),
                ("arduino:avr:mega", "Arduino Mega or Mega 2560"),
                ("arduino:avr:leonardo", "Arduino Leonardo"),
                ("arduino:samd:mkr1000", "Arduino MKR1000"),
                ("esp8266:esp8266:generic", "Generic ESP8266 Module"),
                ("esp32:esp32:esp32", "ESP32 Dev Module"),
                ("rp2040:rp2040:rpipico", "Raspberry Pi Pico"),
            ];
            for (fqbn, label) in defaults {
                items.push(PickItem::new(fqbn, label).description(fqbn));
            }
        }

        let Some(chosen) = host::pick("Select Arduino Board", &items)? else {
            return Ok(None);
        };

        storage::set("board", &chosen)?;
        let name = items
            .iter()
            .find(|i| i.key == chosen)
            .map_or(chosen.as_str(), |i| i.label.as_str());
        host::notify(
            NotificationLevel::Info,
            "Arduino Board Selected",
            &format!("{name} ({chosen})"),
        );
        Ok(Some(chosen))
    }

    fn select_port(&self) -> Result<Option<String>, Error> {
        let mut items = Vec::new();

        if let Ok(output) = host::run_process(
            "arduino-cli",
            &[
                "board".into(),
                "list".into(),
                "--format".into(),
                "json".into(),
            ],
            None,
        ) && output.status == Some(0)
        {
            let parsed: Result<BoardListJson, _> = serde_json::from_str(&output.stdout);
            let port_list: Vec<DetectedPortDetails> = match parsed {
                Ok(BoardListJson::Wrapped(w)) => w
                    .detected_ports
                    .into_iter()
                    .map(|e| e.into_details())
                    .collect(),
                Ok(BoardListJson::Legacy(l)) => l.into_iter().map(|e| e.into_details()).collect(),
                Err(_) => Vec::new(),
            };
            for p in port_list {
                if p.address.trim().is_empty() {
                    continue;
                }
                let board_name = p
                    .boards
                    .first()
                    .map(|b| b.name.as_str())
                    .unwrap_or("Serial Device");
                let label = if p.label.is_empty() {
                    format!("{} ({board_name})", p.address)
                } else {
                    format!("{} ({})", p.label, p.address)
                };
                items.push(PickItem::new(&p.address, &label).description(&p.protocol));
            }
        }

        // Add manual entry option
        items.push(
            PickItem::new("__manual__", "Enter Port Manually…")
                .description("Type COM port or device path"),
        );

        let Some(chosen) = host::pick("Select Serial Port", &items)? else {
            return Ok(None);
        };

        let final_port = if chosen == "__manual__" {
            let Some(input) = host::input(
                "Enter Serial Port (e.g. COM3 or /dev/ttyUSB0)",
                self.current_port().as_deref(),
            )?
            else {
                return Ok(None);
            };
            sanitize_port(&input)
        } else {
            sanitize_port(&chosen)
        };

        if !final_port.is_empty() {
            storage::set("port", &final_port)?;
            host::notify(
                NotificationLevel::Info,
                "Arduino Port Selected",
                &format!("Connected to {final_port}"),
            );
            return Ok(Some(final_port));
        }

        Ok(None)
    }

    fn active_sketch_path(&self, ctx: &Invocation) -> Result<String, Error> {
        if let Some(editor) = ctx.editor() {
            let p = editor.path();
            if p.to_ascii_lowercase().ends_with(".ino") {
                return Ok(p);
            }
        }
        if let Some(project) = ctx.project()
            && let Ok(root) = project.root()
        {
            return Ok(root);
        }
        if let Some(editor) = ctx.editor() {
            let p = editor.path();
            if !p.is_empty() {
                return Ok(p);
            }
        }
        Err(Error::Failed("No active sketch or open project".into()))
    }

    fn build_sketch(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let board = match self.current_board() {
            Some(b) => b,
            None => match self.select_board()? {
                Some(b) => b,
                None => return Ok(None),
            },
        };

        let sketch = self.active_sketch_path(ctx)?;
        let args = vec!["compile".into(), "--fqbn".into(), board.clone(), sketch];

        host::open_terminal("Arduino Build", "arduino-cli", &args)?;
        Ok(Some(format!("Build started for {board}")))
    }

    fn upload_sketch(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let board = match self.current_board() {
            Some(b) => b,
            None => match self.select_board()? {
                Some(b) => b,
                None => return Ok(None),
            },
        };

        let port = match self.current_port() {
            Some(p) => sanitize_port(&p),
            None => match self.select_port()? {
                Some(p) => sanitize_port(&p),
                None => return Ok(None),
            },
        };

        let sketch = self.active_sketch_path(ctx)?;
        let args = vec![
            "upload".into(),
            "-p".into(),
            port.clone(),
            "--fqbn".into(),
            board.clone(),
            sketch,
        ];

        host::open_terminal("Arduino Upload", "arduino-cli", &args)?;
        Ok(Some(format!("Upload started to {port} ({board})")))
    }

    fn serial_monitor(&mut self, _ctx: &Invocation) -> Result<Option<String>, Error> {
        let port = match self.current_port() {
            Some(p) => sanitize_port(&p),
            None => match self.select_port()? {
                Some(p) => sanitize_port(&p),
                None => return Ok(None),
            },
        };

        let bauds = [
            ("9600", "9600 baud (standard)"),
            ("115200", "115200 baud (high speed)"),
            ("57600", "57600 baud"),
            ("38400", "38400 baud"),
            ("19200", "19200 baud"),
            ("4800", "4800 baud"),
        ];
        let items: Vec<_> = bauds
            .iter()
            .map(|(b, desc)| PickItem::new(b, desc))
            .collect();

        let Some(baud) = host::pick(&format!("Select Baud Rate for {port}"), &items)? else {
            return Ok(None);
        };

        let mut args = vec!["monitor".into(), "-p".into(), port.clone()];
        if let Some(board) = self.current_board() {
            args.push("-b".into());
            args.push(board);
        }
        args.push("-c".into());
        args.push(format!("baudrate={baud}"));

        host::open_terminal(
            &format!("Serial Monitor — {port} ({baud} baud)"),
            "arduino-cli",
            &args,
        )?;
        Ok(Some(format!("Serial monitor opened on {port}")))
    }
}

register_plugin!(ArduinoPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn commands_fail_when_missing_capabilities() {
        let host = MockHost::install().with_capabilities(&[]);
        host.answer_next_prompt(Some(&["arduino:avr:uno"]));
        let mut plugin = ArduinoPlugin;
        let inv = host.invocation();
        assert!(matches!(
            plugin.run_command("select-board", &inv, None),
            Err(Error::Denied(_))
        ));
    }

    #[test]
    fn board_selection_saves_to_storage() {
        let host = MockHost::install().with_capabilities(&["storage", "process:arduino-cli"]);
        host.answer_next_prompt(Some(&["arduino:avr:uno"]));
        let mut plugin = ArduinoPlugin;
        let inv = host.invocation();
        let res = plugin.run_command("select-board", &inv, None).unwrap();
        assert_eq!(res, Some("arduino:avr:uno".into()));
        assert_eq!(
            storage::get("board").unwrap(),
            Some("arduino:avr:uno".into())
        );
    }

    #[test]
    fn sanitize_port_normalizes_com_and_preserves_unix() {
        assert_eq!(sanitize_port(" com 14 "), "COM14");
        assert_eq!(sanitize_port("COM3"), "COM3");
        assert_eq!(sanitize_port(" /dev/ttyUSB0 "), "/dev/ttyUSB0");
    }

    #[test]
    fn port_selection_parses_wrapped_and_legacy_board_list_json() {
        use openidea_plugin::ProcessOutput;

        for (payload, expected_pick) in [
            (
                r#"{"detected_ports":[{"port":{"address":" COM 14 ","protocol":"serial","boards":[{"name":"Arduino Uno"}]}}]}"#,
                " COM 14 ",
            ),
            (
                r#"[{"address":"COM14","protocol":"serial","boards":[{"name":"Arduino Uno"}]}]"#,
                "COM14",
            ),
        ] {
            let host = MockHost::install().with_capabilities(&["storage", "process:arduino-cli"]);
            let json = payload.to_owned();
            host.on_process("arduino-cli", move |args, _| {
                assert_eq!(args, &["board", "list", "--format", "json"]);
                Ok(ProcessOutput {
                    status: Some(0),
                    stdout: json.clone(),
                    stderr: String::new(),
                })
            });
            host.answer_next_prompt(Some(&[expected_pick]));
            let mut plugin = ArduinoPlugin;
            let inv = host.invocation();
            let res = plugin.run_command("select-port", &inv, None).unwrap();
            assert_eq!(res, Some("COM14".into()));
            assert_eq!(storage::get("port").unwrap(), Some("COM14".into()));
        }
    }

    #[test]
    fn build_and_upload_open_arduino_cli_terminals() {
        let host = MockHost::install().with_capabilities(&[
            "project.read",
            "storage",
            "process:arduino-cli",
        ]);
        host.set_project_root(Some("/tmp/Blink"));
        host.open_editor("/tmp/Blink/Blink.ino", "arduino", "void setup() {}", &[]);
        storage::set("board", "arduino:avr:uno").unwrap();
        storage::set("port", "com14").unwrap();

        let mut plugin = ArduinoPlugin;
        let inv = host.invocation();
        plugin.run_command("build", &inv, None).unwrap();
        plugin.run_command("upload", &inv, None).unwrap();

        let terms = host.terminals();
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].program, "arduino-cli");
        assert_eq!(
            terms[0].args,
            vec![
                "compile",
                "--fqbn",
                "arduino:avr:uno",
                "/tmp/Blink/Blink.ino"
            ]
        );
        assert_eq!(terms[1].program, "arduino-cli");
        assert_eq!(
            terms[1].args,
            vec![
                "upload",
                "-p",
                "COM14",
                "--fqbn",
                "arduino:avr:uno",
                "/tmp/Blink/Blink.ino"
            ]
        );
    }

    #[test]
    fn serial_monitor_includes_board_when_selected() {
        let host = MockHost::install().with_capabilities(&["storage", "process:arduino-cli"]);
        storage::set("board", "arduino:avr:uno").unwrap();
        storage::set("port", "COM14").unwrap();
        host.answer_next_prompt(Some(&["115200"]));

        let mut plugin = ArduinoPlugin;
        let inv = host.invocation();
        plugin.run_command("serial-monitor", &inv, None).unwrap();

        let terms = host.terminals();
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].program, "arduino-cli");
        assert_eq!(
            terms[0].args,
            vec![
                "monitor",
                "-p",
                "COM14",
                "-b",
                "arduino:avr:uno",
                "-c",
                "baudrate=115200"
            ]
        );
    }

    #[test]
    fn build_falls_back_to_project_root_for_non_ino_editor() {
        let host = MockHost::install().with_capabilities(&[
            "project.read",
            "storage",
            "process:arduino-cli",
        ]);
        host.set_project_root(Some("/tmp/Blink"));
        host.open_editor("/tmp/Blink/helper.h", "c", "#pragma once", &[]);
        storage::set("board", "arduino:avr:uno").unwrap();

        let mut plugin = ArduinoPlugin;
        let inv = host.invocation();
        plugin.run_command("build", &inv, None).unwrap();

        let terms = host.terminals();
        assert_eq!(terms.len(), 1);
        assert_eq!(
            terms[0].args,
            vec!["compile", "--fqbn", "arduino:avr:uno", "/tmp/Blink"]
        );
    }

    #[test]
    fn select_port_manual_input_sanitizes_and_saves() {
        let host = MockHost::install().with_capabilities(&["storage", "process:arduino-cli"]);
        host.answer_next_prompt(Some(&["__manual__"]));
        host.answer_next_prompt(Some(&[" com 7 "]));

        let mut plugin = ArduinoPlugin;
        let inv = host.invocation();
        let res = plugin.run_command("select-port", &inv, None).unwrap();
        assert_eq!(res, Some("COM7".into()));
        assert_eq!(storage::get("port").unwrap(), Some("COM7".into()));
    }

    #[test]
    fn serial_monitor_cancel_baud_does_not_open_terminal() {
        let host = MockHost::install().with_capabilities(&["storage", "process:arduino-cli"]);
        storage::set("port", "COM14").unwrap();
        host.answer_next_prompt(None);

        let mut plugin = ArduinoPlugin;
        let inv = host.invocation();
        let res = plugin.run_command("serial-monitor", &inv, None).unwrap();
        assert_eq!(res, None);
        assert!(host.terminals().is_empty());
    }
}
