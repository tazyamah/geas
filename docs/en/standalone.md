# The windowed exe (standalone app)

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

`geas rust app.geas -o out-dir --host --runtime --standalone` places, in `window/` of the output folder, the crate of an executable `geas_window` that puts the screens and the deeds into one.

```
cd out-dir/window && cargo build --release
./target/release/geas_window
```

It opens the screen in a window of the OS's WebView (WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux). It uses no server, no browser and no port.

## Communication

- **Calls from the screen to deeds do not go through HTTP.** If `window.ipc` exists, the screen (`ui.js`) passes both queries and deeds over the WebView's IPC (inside the process). URLs (`/orders`, etc.) are names "as the definition of a function", as declared in `resource`, not a transport. In an ordinary browser, the same screen works with `fetch`.
- Only the delivery of the screen's files uses the WebView's custom protocol (`geas://`; on Windows WebView2 makes it look like `http://geas.localhost`). It loads only the HTML, JS and CSS embedded in the exe, and does not go out to the network.
- The screen inside the window is given no destination to communicate with (CSP `connect-src 'none'`, and navigation of the window is only to its own screen).
- On Linux, `tests/test_window.py` (kept at the tag `python-final`; needs xvfb and WebKitGTK) used to confirm that the window and its helper processes hold no IP sockets at all (neither listening nor connected). This check has not been moved over after the Rust port (a remaining item).
- Communication that goes outside is made from the Rust side to the declared destination only by external effects that declare `over http { … }` in their `effect` (redirects are not followed; the syntax is in "Dealing with the outside" of [language-reference.md](language-reference.md)). In an app with no such declaration, the communication part (ureq) is in neither the dependencies nor the executable. The destinations it may communicate with appear in the specification's and review screen's "communication going outside" table and in the reachability row of "runtime requirements". The sample is `tests/fixtures/solo_tweet` (posting to X; the test is a local mock of X).

- File operations are done from the Rust side, on the declared place and file, only by external effects that declare `over file` in their `effect` ("File operations" in [language-reference.md](language-reference.md)). An external effect without the declaration does not touch files (the file-handling part is std only, so the dependencies do not grow).

## Resident

Declaring `resident { … }` keeps the app in the task tray (the menu bar on macOS). Closing the window does not end the app but hides it, and it is shown by clicking the icon or by "Open" in the right-click menu. In the right-click menu you can declare items that open a screen, items that fire an event, and separator lines. The syntax is in "Resident" of [language-reference.md](language-reference.md).

- What has been checked: on Linux (xvfb), that a tray icon can be made, that the window opens and the screen is drawn, and that a second launch with the same storage is refused.
- **What has not been checked:** clicks and menus of an icon on a real tray, the behavior of hiding when the window is closed (a virtual display has no tray), and behavior on Windows and macOS. The setting that hides the Dock icon on macOS (the accessory activation policy) has not been added yet.
- To build it, Linux needs libayatana-appindicator3-dev (at run time too, an environment that displays the tray is needed).
- Auto-start at login and OS notifications do not exist yet.

## Storage location

With `path` in `.geas` (a value that holds the path notation per OS) and `data_folder` ("Path values and the data folder" in [language-reference.md](language-reference.md)), you can declare the folder where the DB, the log and the secrets are placed.

(On Windows the log: `log.txt` is output in the same folder. `--console` can show it in a window.)

It is decided with `--db file`. If omitted, `app.sqlite` is placed in a folder named after the app under the user's data folder (Windows: `%APPDATA%`, macOS: `~/Library/Application Support`, otherwise: `$XDG_DATA_HOME` or `~/.local/share`). `--seed initial-data.json` can also be used (the same as the server). The values used for authenticating external effects are passed with `--secrets file`, with `secrets.json` in the data folder (`{"external.name": "value"}`), or with the environment variable `GEAS_SECRET_<external>_<name>` (uppercase). `--selftest` is a launch for testing that opens the window, confirms the screen is drawn, and exits.

## Structure

- `server/` is divided into a lib (`geas_server`: the processing of routes `http::handle`, storage, and the layer that runs deeds) and the HTTP listener (`main.rs`). The windowed version uses only the lib, and does not use the HTTP listener.
- `http::handle` is a function that returns a response (`Resp`) from a request (`Req`) and does not know how it is carried. The server calls it through tiny_http, and the windowed version calls the same through IPC.
- The windowed version's `--test` entrance (clock, mock operations) cannot be opened.

## Premises and what is unconfirmed

- A Windows exe can be built from Linux with `--target x86_64-pc-windows-gnu` (mingw-w64). The procedure and how to check are in "For checking on Windows" below.
- To build it, Linux needs libwebkit2gtk-4.1-dev and the GTK development files.
- A Windows exe has not been built in this development environment (the WebView2 runtime is needed; it is standard on Windows 11 and must be installed separately on 10). `wry` and `tao` are parts that can be built on Windows, but actual confirmation is not finished.

## For checking on Windows

A Windows exe can be built from Linux (to run and check on a real machine; behavior on Windows has not been checked in this development environment).

```
rustup target add x86_64-pc-windows-gnu          # Debian/Ubuntu also needs gcc-mingw-w64-x86-64
geas rust app.geas -o out-dir --host --runtime --standalone
cd out-dir/window
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo build --release --target x86_64-pc-windows-gnu
# the result: target/x86_64-pc-windows-gnu/release/geas_window.exe
# when built with mingw, WebView2Loader.dll is needed as a separate file. Put it in the same folder as the exe:
#   target/x86_64-pc-windows-gnu/release/build/webview2-com-sys-*/out/x64/WebView2Loader.dll
```

The Windows exe starts without showing a terminal window (a windowed-only app). The log (standard error) is appended to `log.txt` in the data folder. When you want the log in a terminal window, add `--console` (if started from a terminal it goes there, otherwise a new window is opened; `--selftest` includes `--console`).

If `WebView2Loader.dll` is not next to the exe, startup says "WebView2Loader.dll was not found" (confirmed on a real Windows machine). If built with MSVC, the dll is statically included in the exe. Because it is an unsigned exe, an "unknown publisher" warning appears.

On the Windows side, the WebView2 runtime is needed (standard on Windows 11). The order of checking is `geas_window.exe --selftest` (exit code 0 if the window opens and the screen is drawn), a normal launch, and (for an app that declares resident) clicking the tray icon, the right-click menu and the behavior of hiding when the window is closed. The storage location is `%APPDATA%\app-name\app.sqlite`.
