//! OpenIDEA Arduino Plugin: Build, Upload and Serial Monitor commands
//! with interactive board and serial port pickers via Arduino CLI.

use openidea_plugin::host::{self, storage};
use openidea_plugin::{Error, Invocation, NotificationLevel, PickItem, Plugin, register_plugin};
use serde::Deserialize;

#[derive(Default)]
pub struct ArduinoPlugin;

#[allow(dead_code)]
#[derive(Clone, Deserialize)]
struct DetectedPortBoard {
    name: String,
    fqbn: String,
}

#[allow(dead_code)]
#[derive(Clone, Default, Deserialize)]
struct DetectedPortDetails {
    #[serde(default)]
    address: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    protocol_label: String,
    #[serde(default)]
    boards: Vec<DetectedPortBoard>,
}

#[allow(dead_code)]
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
    protocol_label: String,
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
                protocol_label: self.protocol_label,
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
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
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
        storage::get("board").ok().flatten().filter(|s| !s.trim().is_empty())
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
        if let Ok(output) = host::run_process("arduino-cli", &["board".into(), "listall".into(), "--format".into(), "json".into()], None)
            && output.status == Some(0)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&output.stdout)
            && let Some(boards) = val.get("boards").and_then(|b| b.as_array())
        {
            for b in boards {
                if let (Some(name), Some(fqbn)) = (b.get("name").and_then(|n| n.as_str()), b.get("fqbn").and_then(|f| f.as_str())) {
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
        let name = items.iter().find(|i| i.key == chosen).map_or(chosen.as_str(), |i| i.label.as_str());
        host::notify(
            NotificationLevel::Info,
            "Arduino Board Selected",
            &format!("{name} ({chosen})"),
        );
        Ok(Some(chosen))
    }

    fn select_port(&self) -> Result<Option<String>, Error> {
        let mut items = Vec::new();

        if let Ok(output) = host::run_process("arduino-cli", &["board".into(), "list".into(), "--format".into(), "json".into()], None)
            && output.status == Some(0)
        {
            let parsed: Result<BoardListJson, _> = serde_json::from_str(&output.stdout);
            let port_list: Vec<DetectedPortDetails> = match parsed {
                Ok(BoardListJson::Wrapped(w)) => w.detected_ports.into_iter().map(|e| e.into_details()).collect(),
                Ok(BoardListJson::Legacy(l)) => l.into_iter().map(|e| e.into_details()).collect(),
                Err(_) => Vec::new(),
            };
            for p in port_list {
                let board_name = p.boards.first().map(|b| b.name.as_str()).unwrap_or("Serial Device");
                let label = if p.label.is_empty() {
                    format!("{} ({board_name})", p.address)
                } else {
                    format!("{} ({})", p.label, p.address)
                };
                items.push(PickItem::new(&p.address, &label).description(&p.protocol));
            }
        }

        // Add manual entry option
        items.push(PickItem::new("__manual__", "Enter Port Manually…").description("Type COM port or device path"));

        let Some(chosen) = host::pick("Select Serial Port", &items)? else {
            return Ok(None);
        };

        let final_port = if chosen == "__manual__" {
            let Some(input) = host::input("Enter Serial Port (e.g. COM3 or /dev/ttyUSB0)", self.current_port().as_deref())? else {
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
            if !p.is_empty() {
                return Ok(p);
            }
        }
        if let Some(project) = ctx.project()
            && let Ok(root) = project.root()
        {
            return Ok(root);
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
        let args = vec![
            "compile".into(),
            "--fqbn".into(),
            board.clone(),
            sketch,
        ];

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
        let items: Vec<_> = bauds.iter().map(|(b, desc)| PickItem::new(*b, *desc)).collect();

        let baud = host::pick(&format!("Select Baud Rate for {port}"), &items)?.unwrap_or_else(|| "9600".into());

        let args = vec![
            "monitor".into(),
            "-p".into(),
            port.clone(),
            "-c".into(),
            format!("baudrate={baud}"),
        ];

        host::open_terminal(&format!("Serial Monitor — {port} ({baud} baud)"), "arduino-cli", &args)?;
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
        let mut plugin = ArduinoPlugin::default();
        let inv = host.invocation();
        assert!(matches!(plugin.run_command("select-board", &inv, None), Err(_)));
    }

    #[test]
    fn board_selection_saves_to_storage() {
        let host = MockHost::install().with_capabilities(&["storage", "process:arduino-cli"]);
        host.answer_next_prompt(Some(&["arduino:avr:uno"]));
        let mut plugin = ArduinoPlugin::default();
        let inv = host.invocation();
        let res = plugin.run_command("select-board", &inv, None).unwrap();
        assert_eq!(res, Some("arduino:avr:uno".into()));
        assert_eq!(storage::get("board").unwrap(), Some("arduino:avr:uno".into()));
    }
}
