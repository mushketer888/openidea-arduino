# OpenIDEA Arduino Plugin

Arduino integration for [OpenIDEA](https://github.com/l0ng-ai/openidea):
- **Build Sketch**: Compiles the active `.ino` sketch with the configured board via `arduino-cli`.
- **Upload Sketch**: Compiles and uploads sketch to connected microcontroller board.
- **Serial Monitor**: Opens interactive serial monitor tab with selectable baud rate (`9600`, `115200`, etc.).
- **Board & Port Pickers**: Interactive pickers with automatic board listall and connected serial port discovery.

## Requirements

- [OpenIDEA](https://github.com/l0ng-ai/openidea)
- [Arduino CLI](https://arduino.github.io/arduino-cli/) or [Arduino IDE 2](https://www.arduino.cc/en/software)

## Commands

- `Arduino: Select Board` (`mushketer888.arduino.select-board`)
- `Arduino: Select Port` (`mushketer888.arduino.select-port`)
- `Arduino: Build Sketch` (`mushketer888.arduino.build`)
- `Arduino: Upload Sketch...` (`mushketer888.arduino.upload`)
- `Arduino: Serial Monitor...` (`mushketer888.arduino.serial-monitor`)

## Screenshots

### Build Sketch
![Build Sketch](screenshots/02-arduino-build-sketch-success.png)

### Serial Monitor Config
![Serial Monitor Config](screenshots/04-arduino-serial-monitor-dialog.png)

### Serial Monitor Live Data
![Serial Monitor Live Data](screenshots/05-arduino-serial-monitor-live-data.png)

## License

[Apache 2.0](LICENSE)
