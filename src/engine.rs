use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TEXT: &str = "\x1b[38;5;203m";
const LINK: &str = "\x1b[38;5;220m";
const FIELD: &str = "\x1b[38;5;51m";
const DIM: &str = "\x1b[38;5;131m";

pub const KIND_LINK: u8 = 1;
pub const KIND_FIELD: u8 = 2;
pub const KIND_BUTTON: u8 = 3;

#[derive(Clone, Debug)]
pub struct WebHit {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    /// Cell rows the control occupies. A click on any of them hits this control.
    pub h: usize,
    pub kind: u8,
    #[cfg_attr(not(test), allow(dead_code))]
    pub href: String,
    pub label: String,
    pub px: i32,
    pub py: i32,
}

pub struct WebView {
    pub url: String,
    pub lines: Vec<String>,
    pub hits: Vec<WebHit>,
}

struct Node {
    kind: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    text: String,
    extra: String,
    align: char,
    fg: String,
    bg: String,
    bd: String,
}

#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    ink: &'static str,
    fg: Option<u32>,
    bg: Option<u32>,
}

struct Blot {
    col: usize,
    row: usize,
    cols: usize,
    lines: Vec<String>,
}

struct Shot {
    key: String,
    picture: String,
}

pub struct Engine {
    child: Option<Child>,
    stream: Option<TcpStream>,
    id: u64,
    shots: Vec<Shot>,
    dir: PathBuf,
    pending: Vec<u8>,
    view_cols: usize,
    view_rows: usize,
    dark: bool,
    locale: String,
    look_ready: bool,
    js: bool,
    live: bool,
    focus_x: i32,
    focus_y: i32,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            child: None,
            stream: None,
            id: 0,
            shots: Vec::new(),
            dir: PathBuf::new(),
            pending: Vec::new(),
            view_cols: 0,
            view_rows: 0,
            dark: false,
            locale: "en-US".into(),
            look_ready: false,
            js: true,
            live: true,
            focus_x: 0,
            focus_y: 0,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Engine {
    pub fn show(&mut self, url: &str, cols: usize, rows: usize) -> Result<WebView, String> {
        self.ensure()?;
        self.ensure_look()?;
        let (cols, rows, view_rows) = fit_view(cols, rows);
        self.metrics(cols, view_rows)?;
        self.view_cols = cols;
        self.view_rows = view_rows;
        let url = prefer_google_lang(url, &self.locale);
        self.prepare_net();
        self.go(&url)?;
        self.shots.clear();
        let view = self.paint(&url, cols, rows, true)?;
        self.seal_policy();
        Ok(view)
    }

    pub fn refresh(&mut self, cols: usize, rows: usize, capture: bool) -> Result<WebView, String> {
        self.ensure()?;
        self.ensure_look()?;
        let (cols, rows, view_rows) = fit_view(cols, rows);
        let mut capture = capture;
        if self.view_cols != cols || self.view_rows != view_rows {
            self.metrics(cols, view_rows)?;
            self.view_cols = cols;
            self.view_rows = view_rows;
            self.shots.clear();
            capture = true;
        }
        let href = self.href().unwrap_or_default();
        self.paint(&href, cols, rows, capture)
    }

    pub fn focus_at(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.ensure()?;
        self.focus_x = x;
        self.focus_y = y;
        let answer = self.eval(&focus_js(x, y), false)?;
        if let Some((cx, cy)) = split_point(&answer) {
            self.focus_x = cx;
            self.focus_y = cy;
            self.click(cx, cy)?;
            return Ok(());
        }
        // A combobox or a shadow editor has no light-dom box at this point.
        // The click still puts the caret where the page keeps it.
        self.click(x, y)?;
        let _ = answer;
        Ok(())
    }

    pub fn click(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.ensure()?;
        for (kind, buttons) in [("mousePressed", 1), ("mouseReleased", 0)] {
            let params = format!(
                r#"{{"type":"{kind}","x":{x},"y":{y},"button":"left","buttons":{buttons},"clickCount":1}}"#
            );
            self.call("Input.dispatchMouseEvent", &params)?;
        }
        Ok(())
    }

    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        self.ensure()?;
        if text.is_empty() {
            return Ok(());
        }
        let answer = self.eval(&field_js(self.focus_x, self.focus_y, text, false), false)?;
        if answer == "ok" {
            return Ok(());
        }
        // The editor lives in a shadow root. A real keystroke still lands.
        if answer == "none" {
            let params = format!(r#"{{"text":{}}}"#, json_str(text));
            self.call("Input.insertText", &params)?;
            return Ok(());
        }
        Err(answer)
    }

    pub fn key(&mut self, key: &str, code: i32) -> Result<(), String> {
        self.ensure()?;
        if key == "Backspace" {
            let answer = self.eval(&field_js(self.focus_x, self.focus_y, "", true), false)?;
            if answer == "ok" {
                return Ok(());
            }
        }
        let text = if key == "Enter" {
            r#","text":"\r""#
        } else if key == "Backspace" {
            r#","text":"\u0008""#
        } else {
            ""
        };
        for typ in ["keyDown", "keyUp"] {
            let params = format!(
                r#"{{"type":"{typ}","key":"{key}","code":"{key}","windowsVirtualKeyCode":{code}{text}}}"#
            );
            self.call("Input.dispatchKeyEvent", &params)?;
        }
        Ok(())
    }

    /// Stores the page policy. It does not start the browser.
    pub fn set_policy(&mut self, js: bool, live: bool) {
        self.js = js;
        self.live = live;
    }

    /// Document bytes, then document plus resources. Fails when the browser is down.
    pub fn traffic(&mut self) -> Result<(u64, u64), String> {
        if self.stream.is_none() {
            return Err("no browser".into());
        }
        let raw = self.eval(TRAFFIC, false)?;
        let mut parts = raw.split('\t');
        let page = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let all = parts.next().unwrap_or("0").parse().unwrap_or(page);
        Ok((page, all))
    }

    /// HTML length when the navigation timing counters stayed at zero.
    pub fn document_bytes(&mut self) -> Result<u64, String> {
        if self.stream.is_none() {
            return Err("no browser".into());
        }
        let raw = self.eval(
            "(document.documentElement&&document.documentElement.outerHTML.length)||0",
            false,
        )?;
        Ok(raw.trim().parse().unwrap_or(0))
    }

    pub fn set_look(&mut self, dark: bool, locale: &str) {
        let locale = if locale.is_empty() {
            "en-US".to_string()
        } else {
            locale.to_string()
        };
        if self.dark != dark || self.locale != locale {
            self.dark = dark;
            self.locale = locale;
            self.look_ready = false;
        }
    }

    pub fn reload(&mut self) -> Result<(), String> {
        self.ensure()?;
        self.look_ready = false;
        self.ensure_look()?;
        self.prepare_net();
        let href = self.href().unwrap_or_default();
        if href.starts_with("http://") || href.starts_with("https://") {
            let next = prefer_google_lang(&href, &self.locale);
            self.shots.clear();
            self.go(&next)?;
        }
        self.seal_policy();
        Ok(())
    }

    pub fn wheel(&mut self, delta: i32) -> Result<(), String> {
        self.ensure()?;
        self.eval(&wheel_js(wheel_delta(delta)), false)?;
        Ok(())
    }

    pub fn href(&mut self) -> Result<String, String> {
        self.eval("location.href", false)
    }

    pub fn wait(&mut self) -> Result<(), String> {
        let expr = "new Promise(function(r){var n=0;function tick(){n++;var box=document.querySelector('input,textarea,button,h1,a[href],[role=combobox],[role=searchbox]');var logo=false;var pics=document.querySelectorAll('img,svg,canvas');for(var i=0;i<pics.length&&i<80;i++){var b=pics[i].getBoundingClientRect();if(b.width>=24&&b.height>=24&&b.bottom>0&&b.top<innerHeight){logo=true;break;}}if((document.readyState==='complete'&&box&&n>2&&(logo||n>8))||n>30){r('ok');return;}setTimeout(tick,150);}tick();})";
        self.eval(expr, true)?;
        Ok(())
    }

    fn paint(&mut self, url: &str, cols: usize, rows: usize, capture: bool) -> Result<WebView, String> {
        let (cols, rows, view_rows) = fit_view(cols, rows);
        let raw = self.eval(LAYOUT, false)?;
        let nodes = parse_nodes(&raw);
        let mut order: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.kind == "img")
            .map(|(index, _)| index)
            .collect();
        order.sort_by_key(|&index| {
            let node = &nodes[index];
            std::cmp::Reverse((node.w.max(0) as i64) * (node.h.max(0) as i64))
        });
        order.truncate(8);
        let mut shots = Vec::new();
        let mut taken = 0usize;
        let fetch_until = Instant::now()
            + if capture {
                Duration::from_secs(4)
            } else {
                Duration::from_millis(900)
            };
        let page_url = if url.starts_with("http") {
            url.to_string()
        } else {
            self.href().unwrap_or_default()
        };
        for (index, node) in nodes.iter().enumerate() {
            if node.kind != "img" || !order.contains(&index) {
                shots.push(None);
                continue;
            }
            let chosen = crate::page::image_choice(&node.text, &node.extra, &page_url);
            let key = chosen.clone().unwrap_or_else(|| shot_key(node));
            if let Some(found) = self.shots.iter().find(|shot| shot.key == key) {
                shots.push(Some(found.picture.clone()));
                continue;
            }
            let budget = if capture { 6 } else { 4 };
            if taken >= budget || Instant::now() >= fetch_until {
                shots.push(None);
                continue;
            }
            taken += 1;
            let picture = self.draw_image(node, chosen.as_deref().unwrap_or(""), cols, view_rows);
            if let Some(picture) = &picture {
                self.shots.push(Shot {
                    key,
                    picture: picture.clone(),
                });
                if self.shots.len() > 12 {
                    self.shots.remove(0);
                }
            }
            shots.push(picture);
        }
        let wash = nodes.iter().find(|node| node.kind == "wash");
        let wash_fg = wash.and_then(|node| rgb(&node.fg));
        let wash_bg = wash.and_then(|node| rgb(&node.bg));
        let mut view = compose(&nodes, &shots, cols, view_rows);
        let shown = if url.is_empty() { self.href().unwrap_or_default() } else { url.to_string() };
        let head = address_line(&shown, wash_fg, wash_bg);
        view.lines.insert(0, head);
        for hit in &mut view.hits {
            hit.y += 1;
        }
        view.lines.truncate(rows);
        view.hits.retain(|hit| hit.y < rows && hit.w > 0 && hit.h > 0);
        for hit in &mut view.hits {
            let room = rows.saturating_sub(hit.y).max(1);
            if hit.h > room {
                hit.h = room;
            }
        }
        view.url = if shown.starts_with("http") || shown.starts_with("data:") {
            shown
        } else {
            self.href().unwrap_or(shown)
        };
        Ok(view)
    }

    fn draw_image(&mut self, node: &Node, url: &str, cols: usize, rows: usize) -> Option<String> {
        let (_, _, cell_w, cell_h) = cell_box(node, cols, rows)?;
        if !url.is_empty()
            && let Some(bytes) = crate::page::load_raster_limited(url, 2)
            && let Some(picture) = picture_of(&bytes, cell_w, cell_h)
        {
            return Some(picture);
        }
        if node.w > 512 || node.h > 512 {
            return None;
        }
        // An inline SVG has no file to fetch. Draw it in the page, then fall
        // back to a clipped screenshot when the clone loses a paint server.
        if matches!(node.text.as_str(), "SVG" | "CANVAS")
            && let Some(bytes) = self.vector_png(node)
            && let Some(picture) = picture_of(&bytes, cell_w, cell_h)
        {
            return Some(picture);
        }
        for surface in [false, true] {
            if let Some(bytes) = self.clip_png(node, cols, rows, surface)
                && let Some(picture) = picture_of(&bytes, cell_w, cell_h)
            {
                return Some(picture);
            }
        }
        None
    }

    /// Rasterize the svg or canvas that occupies this box. A transparent
    /// wordmark has no pixels for elementFromPoint, so the screenshot of that
    /// box is often a blank white tile.
    fn vector_png(&mut self, node: &Node) -> Option<Vec<u8>> {
        let x = node.x.max(0);
        let y = node.y.max(0);
        let w = node.w.max(1);
        let h = node.h.max(1);
        let expr = format!(
            "new Promise(function(resolve){{function done(v){{resolve(v||'');}}var list=document.querySelectorAll('svg,canvas');var el=null,best=1e9;for(var i=0;i<list.length;i++){{var r=list[i].getBoundingClientRect();var d=Math.abs(r.left-{x})+Math.abs(r.top-{y})+Math.abs(r.width-{w})+Math.abs(r.height-{h});if(d<best){{best=d;el=list[i];}}}}if(!el||best>24){{done('');return;}}var box=el.getBoundingClientRect();var pw=Math.max(1,Math.min(512,Math.round(box.width)));var ph=Math.max(1,Math.min(512,Math.round(box.height)));if(el.tagName==='CANVAS'){{try{{done(el.toDataURL('image/png'));}}catch(e){{done('');}}return;}}var clone=el.cloneNode(true);clone.setAttribute('xmlns','http://www.w3.org/2000/svg');clone.setAttribute('width',String(pw));clone.setAttribute('height',String(ph));var xml=new XMLSerializer().serializeToString(clone);var img=new Image();img.onload=function(){{try{{var c=document.createElement('canvas');c.width=pw;c.height=ph;var g=c.getContext('2d');g.fillStyle='#ffffff';g.fillRect(0,0,pw,ph);g.drawImage(img,0,0,pw,ph);done(c.toDataURL('image/png'));}}catch(e){{done('');}}}};img.onerror=function(){{done('');}};img.src='data:image/svg+xml;charset=utf-8,'+encodeURIComponent(xml);}})"
        );
        let raw = self.eval(&expr, true).ok()?;
        let bytes = png_bytes(&raw)?;
        (bytes.len() >= 32 && bytes.len() <= 400_000).then_some(bytes)
    }

    fn clip_png(&mut self, node: &Node, cols: usize, rows: usize, surface: bool) -> Option<Vec<u8>> {
        let x = node.x.max(0);
        let y = node.y.max(0);
        let w = node.w.clamp(8, cols as i32 * 8);
        let h = node.h.clamp(8, rows as i32 * 16);
        let flag = if surface { "true" } else { "false" };
        let params = format!(
            r#"{{"format":"png","fromSurface":{flag},"clip":{{"x":{x},"y":{y},"width":{w},"height":{h},"scale":1}}}}"#
        );
        let raw = self.call("Page.captureScreenshot", &params).ok()?;
        let data = json_field(&raw, "data")?;
        let bytes = b64_decode(&data)?;
        (bytes.len() >= 32 && bytes.len() <= 1_500_000).then_some(bytes)
    }

    fn metrics(&mut self, cols: usize, rows: usize) -> Result<(), String> {
        let width = cols * 8;
        let height = rows * 16;
        let params = format!(
            r#"{{"width":{width},"height":{height},"deviceScaleFactor":1,"mobile":false}}"#
        );
        self.call("Emulation.setDeviceMetricsOverride", &params)?;
        Ok(())
    }

    fn ensure_look(&mut self) -> Result<(), String> {
        if self.look_ready {
            return Ok(());
        }
        self.apply_look()?;
        self.look_ready = true;
        Ok(())
    }

    fn apply_look(&mut self) -> Result<(), String> {
        let scheme = if self.dark { "dark" } else { "light" };
        let media = format!(
            r#"{{"media":"","features":[{{"name":"prefers-color-scheme","value":"{scheme}"}}]}}"#
        );
        let _ = self.call("Emulation.setEmulatedMedia", &media);
        let enabled = if self.dark { "true" } else { "false" };
        let _ = self.call(
            "Emulation.setAutoDarkModeOverride",
            &format!(r#"{{"enabled":{enabled}}}"#),
        );
        let _ = self.call(
            "Emulation.setLocaleOverride",
            &format!(r#"{{"locale":{}}}"#, json_str(&self.locale)),
        );
        let _ = self.call("Network.enable", "");
        let accept = accept_language(&self.locale);
        let _ = self.call(
            "Network.setExtraHTTPHeaders",
            &format!(
                r#"{{"headers":{{"Accept-Language":{}}}}}"#,
                json_str(&accept)
            ),
        );
        let raw = self
            .eval("navigator.userAgent", false)
            .unwrap_or_default();
        let ua = plain_agent(&raw);
        let _ = self.call(
            "Emulation.setUserAgentOverride",
            &format!(
                r#"{{"userAgent":{},"acceptLanguage":{},"platform":"Win32"}}"#,
                json_str(&ua),
                json_str(&accept)
            ),
        );
        Ok(())
    }

    /// Network is online and scripts can run, so the document loads and the
    /// layout can be measured. The js and live chips freeze the page after paint.
    fn prepare_net(&mut self) {
        let _ = self.call("Network.enable", "");
        let _ = self.call(
            "Network.emulateNetworkConditions",
            r#"{"offline":false,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1}"#,
        );
        self.apply_scripts(true);
    }

    fn apply_scripts(&mut self, enabled: bool) {
        let off = if enabled { "false" } else { "true" };
        let _ = self.call(
            "Emulation.setScriptExecutionDisabled",
            &format!(r#"{{"value":{off}}}"#),
        );
    }

    /// After the document is painted, live off freezes scripts and the network.
    fn seal_policy(&mut self) {
        self.apply_scripts(self.js && self.live);
        let offline = if self.live { "false" } else { "true" };
        let _ = self.call(
            "Network.emulateNetworkConditions",
            &format!(
                r#"{{"offline":{offline},"latency":0,"downloadThroughput":-1,"uploadThroughput":-1}}"#
            ),
        );
    }

    fn go(&mut self, url: &str) -> Result<(), String> {
        let params = format!(r#"{{"url":{}}}"#, json_str(url));
        self.call("Page.navigate", &params)?;
        self.wait()
    }

    fn ensure(&mut self) -> Result<(), String> {
        if self.alive() {
            return Ok(());
        }
        self.stop();
        self.start()
    }

    /// The debugging socket is the session. Edge's launcher process exits
    /// after the browser is up, so a dead child is not a dead page.
    fn alive(&mut self) -> bool {
        self.stream.is_some()
    }

    pub fn close(&mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.stream.take();
        let dir = std::mem::take(&mut self.dir);
        if !dir.as_os_str().is_empty() {
            end_profile(&dir);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        self.shots.clear();
        self.pending.clear();
        self.view_cols = 0;
        self.view_rows = 0;
        self.look_ready = false;
    }

    fn start(&mut self) -> Result<(), String> {
        let path = browser_path().ok_or_else(|| "Edge or Chrome is not installed".to_string())?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|err| format!("port: {err}"))?;
        let port = listener
            .local_addr()
            .map_err(|err| format!("port: {err}"))?
            .port();
        drop(listener);
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("granttap-edge-{n}"));
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        let log = std::fs::File::create(dir.join("edge.log")).ok();
        let lang_flag = format!(
            "--lang={}",
            if self.locale.is_empty() {
                "en-US"
            } else {
                &self.locale
            }
        );
        let mut cmd = Command::new(&path);
        cmd.args([
            "--headless=new",
            "--disable-blink-features=AutomationControlled",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-sync",
            "--remote-allow-origins=*",
            lang_flag.as_str(),
            &format!("--remote-debugging-port={port}"),
            &format!("--user-data-dir={}", dir.display()),
            "about:blank",
        ]);
        cmd.stdin(Stdio::null()).stdout(Stdio::null());
        if let Some(log) = log {
            cmd.stderr(log);
        } else {
            cmd.stderr(Stdio::null());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let child = cmd.spawn().map_err(|err| format!("edge: {err}"))?;
        self.child = Some(child);
        self.dir = dir;
        let mut last = String::from("edge did not open");
        std::thread::sleep(Duration::from_millis(200));
        let deadline = Instant::now() + Duration::from_secs(12);
        while Instant::now() < deadline {
            match page_list(port) {
                Ok(body) => {
                    let ws = page_socket(&body).ok_or_else(|| "page socket missing".to_string())?;
                    self.attach(&ws)?;
                    self.call("Page.enable", "")?;
                    self.call("Runtime.enable", "")?;
                    let _ = self.call(
                        "Page.addScriptToEvaluateOnNewDocument",
                        &format!(r#"{{"source":{}}}"#, json_str(CLIENT_SCRIPT)),
                    );
                    let _ = self.apply_look();
                    self.look_ready = true;
                    return Ok(());
                }
                Err(err) => last = err,
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        let log = std::fs::read_to_string(self.dir.join("edge.log")).unwrap_or_default();
        let tail: String = log.chars().rev().take(120).collect::<Vec<_>>().into_iter().rev().collect();
        self.stop();
        let shown = if tail.trim().is_empty() {
            last
        } else {
            format!("{last} {}", tail.trim())
        };
        Err(brief_net(&shown))
    }

    fn attach(&mut self, ws: &str) -> Result<(), String> {
        let (host, port, path) = split_ws(ws).ok_or_else(|| format!("bad socket {ws}"))?;
        let mut stream = dial(&host, port)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(12)))
            .map_err(|err| err.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(8)))
            .map_err(|err| err.to_string())?;
        let key = b64(&mask_bytes());
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(req.as_bytes())
            .map_err(|err| err.to_string())?;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut tmp).map_err(|err| err.to_string())?;
            if n == 0 {
                return Err("socket closed".into());
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.len() > 8192 {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf);
        let accept = b64(&sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes()));
        if !head.contains("101") || !head.contains(&accept) {
            return Err(format!("socket refused {}", head.chars().take(80).collect::<String>()));
        }
        let split = buf
            .windows(4)
            .position(|mark| mark == b"\r\n\r\n")
            .map(|at| at + 4)
            .unwrap_or(buf.len());
        self.pending = buf.get(split..).unwrap_or_default().to_vec();
        self.stream = Some(stream);
        Ok(())
    }

    fn eval(&mut self, expr: &str, await_promise: bool) -> Result<String, String> {
        match self.eval_once(expr, await_promise) {
            Err(err) if script_blocked(&err) => {
                self.apply_scripts(true);
                let result = self.eval_once(expr, await_promise);
                if !(self.js && self.live) {
                    self.apply_scripts(false);
                }
                result
            }
            other => other,
        }
    }

    fn eval_once(&mut self, expr: &str, await_promise: bool) -> Result<String, String> {
        let flag = if await_promise { "true" } else { "false" };
        let params = format!(
            r#"{{"expression":{},"returnByValue":true,"awaitPromise":{flag}}}"#,
            json_str(expr)
        );
        let raw = self.call("Runtime.evaluate", &params)?;
        if let Some(text) = json_string_value(&raw) {
            return Ok(text);
        }
        Err(raw.chars().take(240).collect())
    }

    fn call(&mut self, method: &str, params: &str) -> Result<String, String> {
        self.id += 1;
        let id = self.id;
        let msg = if params.is_empty() {
            format!(r#"{{"id":{id},"method":"{method}"}}"#)
        } else {
            format!(r#"{{"id":{id},"method":"{method}","params":{params}}}"#)
        };
        self.send_text(&msg)?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if Instant::now() > deadline {
                return Err(format!("{method} timed out"));
            }
            let incoming = self.read_message()?;
            if response_id(&incoming, id) {
                if let Some(err) = cdp_error(&incoming) {
                    return Err(format!("{method}: {err}"));
                }
                return Ok(incoming);
            }
        }
    }

    fn send_text(&mut self, text: &str) -> Result<(), String> {
        self.send_frame(1, text.as_bytes())
    }

    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        if self.stream.is_none() {
            return Err("no browser".into());
        }
        let mask = mask4();
        let mut frame = Vec::new();
        frame.push(0x80 | opcode);
        let len = payload.len();
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        let wrote = {
            let stream = self.stream.as_mut().ok_or_else(|| "no browser".to_string())?;
            stream.write_all(&frame)
        };
        if let Err(err) = wrote {
            self.stream.take();
            return Err(err.to_string());
        }
        Ok(())
    }

    fn read_message(&mut self) -> Result<String, String> {
        let mut text = String::new();
        loop {
            let (opcode, fin, payload) = self.read_frame()?;
            match opcode {
                1 | 2 | 0 => {
                    text.push_str(&String::from_utf8_lossy(&payload));
                    if fin {
                        return Ok(text);
                    }
                }
                8 => return Err("browser closed the socket".into()),
                9 => self.send_frame(0xA, &payload)?,
                _ => {}
            }
        }
    }

    fn read_exact(&mut self, len: usize) -> Result<Vec<u8>, String> {
        let mut buf = Vec::with_capacity(len);
        if !self.pending.is_empty() {
            let n = len.min(self.pending.len());
            buf.extend(self.pending.drain(..n));
        }
        if buf.len() < len {
            let rest = {
                let stream = self.stream.as_mut().ok_or_else(|| "no browser".to_string())?;
                read_n(stream, len - buf.len())
            };
            match rest {
                Ok(more) => buf.extend(more),
                Err(err) => {
                    self.stream.take();
                    return Err(err);
                }
            }
        }
        Ok(buf)
    }

    fn read_frame(&mut self) -> Result<(u8, bool, Vec<u8>), String> {
        let head = self.read_exact(2)?;
        let fin = head[0] & 0x80 != 0;
        let opcode = head[0] & 0x0f;
        let masked = head[1] & 0x80 != 0;
        let mut len = (head[1] & 0x7f) as u64;
        if len == 126 {
            let ext = self.read_exact(2)?;
            len = u16::from_be_bytes([ext[0], ext[1]]) as u64;
        } else if len == 127 {
            let ext = self.read_exact(8)?;
            len = u64::from_be_bytes(ext.try_into().unwrap_or([0; 8]));
        }
        if len > 12_000_000 {
            return Err("frame too large".into());
        }
        let mask = if masked { Some(self.read_exact(4)?) } else { None };
        let mut payload = self.read_exact(len as usize)?;
        if let Some(mask) = mask {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        Ok((opcode, fin, payload))
    }
}

const CLIENT_SCRIPT: &str = "try{Object.defineProperty(Navigator.prototype,'webdriver',{get:function(){return undefined}});}catch(e){}";

const LAYOUT: &str = r#"(function(){var lines=[],seen={};if(!document.body)return '';function push(k,r,text,extra,align,fg,bg,bd){if(!r||r.width<2||r.height<2)return;if(r.bottom<0||r.right<0||r.top>innerHeight||r.left>innerWidth)return;if(extra==null&&align==null&&String(text).indexOf('\t')>=0){var bits=String(text).split('\t');text=bits[0];extra=bits.slice(1).join(' ');}text=String(text==null?'':text).replace(/[\t\n\r]+/g,' ');extra=String(extra==null?'':extra).replace(/[\t\n\r]+/g,' ');var x=Math.round(r.left),y=Math.round(r.top),w=Math.round(r.width),h=Math.round(r.height);var key=k+'|'+x+'|'+y+'|'+w+'|'+text.slice(0,24);if(seen[key]||lines.length>520)return;seen[key]=1;lines.push([k,x,y,w,h,text,extra,align||'L',fg||'',bg||'',bd||''].join('\t'));}function hex(c){if(!c)return '';var m=String(c).match(/rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)(?:[,\s\/]+([0-9.]+))?\s*\)/);if(!m)return '';if(m[4]!=null&&Number(m[4])<0.35)return '';function h(n){return ('0'+Number(n).toString(16)).slice(-2);}return h(m[1])+h(m[2])+h(m[3]);}function borderOf(el){var s=getComputedStyle(el);var w=parseFloat(s.borderTopWidth)||parseFloat(s.borderLeftWidth)||0;if(w<1)return '';return hex(s.borderTopColor)||hex(s.borderLeftColor)||'';}function pair(el){var s=getComputedStyle(el);return [hex(s.color),hex(s.backgroundColor)];}var bs=getComputedStyle(document.body),rs=getComputedStyle(document.documentElement);var pageBg=hex(bs.backgroundColor)||hex(rs.backgroundColor)||'ffffff';var pageFg=hex(bs.color)||hex(rs.color)||'202124';lines.push(['wash',0,0,Math.round(innerWidth),Math.round(innerHeight),'','','L',pageFg,pageBg,''].join('\t'));function vis(el){var s=getComputedStyle(el);if(s.display==='none'||s.visibility==='hidden'||s.visibility==='collapse'||Number(s.opacity)<0.05||s.contentVisibility==='hidden')return null;var r=el.getBoundingClientRect();if(!r||r.width<2||r.height<2)return null;var x=r.left,y=r.top,w=r.width,h=r.height;for(var a=el.parentElement;a&&a!==document.documentElement;a=a.parentElement){var ps=getComputedStyle(a);var ox=ps.overflowX,oy=ps.overflowY;var clip=ox==='hidden'||ox==='clip'||ox==='auto'||ox==='scroll'||oy==='hidden'||oy==='clip'||oy==='auto'||oy==='scroll';if(!clip)continue;var b=a.getBoundingClientRect();var nx=Math.max(x,b.left),ny=Math.max(y,b.top);var nr=Math.min(x+w,b.right),nb=Math.min(y+h,b.bottom);x=nx;y=ny;w=nr-nx;h=nb-ny;if(w<2||h<2)return null;}return {left:x,top:y,right:x+w,bottom:y+h,width:w,height:h};}function front(el,r){if(!el||!r)return true;var pts=[[r.left+Math.max(1,r.width/2),r.top+Math.max(1,r.height/2)],[r.left+Math.min(8,r.width/2),r.top+Math.min(8,r.height/2)],[r.right-Math.min(8,r.width/2),r.bottom-Math.min(8,r.height/2)]];var seen=0,ok=0;for(var i=0;i<pts.length;i++){var x=pts[i][0],y=pts[i][1];if(x<1||y<1||x>=innerWidth-1||y>=innerHeight-1)continue;var hit=document.elementFromPoint(x,y);if(!hit)continue;seen++;if(hit===el||el.contains(hit)||hit.contains(el))ok++;}if(!seen)return true;return ok*2>=seen;}function alignOf(el){var s=getComputedStyle(el);var a=s.textAlign;if(a==='center')return 'C';if(a==='right'||a==='end')return 'R';if(s.direction==='rtl')return 'R';return 'L';}function shown(el){for(var a=el;a&&a!==document.documentElement;a=a.parentElement){if(a.getAttribute&&(a.getAttribute('aria-hidden')==='true'||a.hasAttribute('hidden')||a.hasAttribute('inert')))return false;var s=getComputedStyle(a);if(s.display==='none'||s.visibility==='hidden'||s.visibility==='collapse'||Number(s.opacity)<0.05||s.contentVisibility==='hidden')return false;}return true;}function painted(el){for(var a=el;a&&a!==document.documentElement;a=a.parentElement){if(a.hasAttribute&&a.hasAttribute('hidden'))return false;var s=getComputedStyle(a);if(s.display==='none'||s.visibility==='hidden'||Number(s.opacity)===0)return false;}return true;}function frontImage(el,r){if(!el||!r)return true;var pts=[[r.left+r.width/2,r.top+r.height/2],[r.left+Math.min(16,r.width/3),r.top+r.height/2],[r.right-Math.min(16,r.width/3),r.top+r.height/2],[r.left+r.width/2,r.top+Math.min(12,r.height/3)],[r.left+r.width/2,r.bottom-Math.min(12,r.height/3)]];var seen=0,ok=0;for(var i=0;i<pts.length;i++){var x=pts[i][0],y=pts[i][1];if(x<1||y<1||x>=innerWidth-1||y>=innerHeight-1)continue;var hit=document.elementFromPoint(x,y);if(!hit)continue;seen++;if(hit===el||el.contains(hit)||hit.contains(el))ok++;}if(!seen)return true;return ok>0;}function linesOf(node,text){var out=[],start=0,guard=0;while(start<text.length&&out.length<12&&guard<40){guard++;var range=document.createRange();try{range.setStart(node,start);range.setEnd(node,text.length);}catch(e){break;}var rects=range.getClientRects();if(!rects.length){start++;continue;}var top=rects[0].top;var lo=start+1,hi=text.length,best=Math.min(text.length,start+1);while(lo<=hi){var mid=(lo+hi)>>1;try{range.setStart(node,start);range.setEnd(node,mid);}catch(e){break;}var rr=range.getClientRects();var ok=rr.length>0;if(ok){for(var li=0;li<rr.length;li++){if(Math.abs(rr[li].top-top)>3){ok=false;break;}}}if(ok){best=mid;lo=mid+1;}else hi=mid-1;}try{range.setStart(node,start);range.setEnd(node,best);}catch(e){break;}var box=range.getBoundingClientRect();var piece=text.slice(start,best);if(piece.trim()&&box.width>=2&&box.height>=2)out.push({box:box,text:piece});if(best<=start)start++;else start=best;}return out;}var nodes=document.querySelectorAll('img,svg,canvas,input,textarea,select,button,a[href],[role=button],[role=link],[role=searchbox],[role=combobox],[role=textbox],[contenteditable=true]');for(var i=0;i<nodes.length;i++){var el=nodes[i],tag=String(el.tagName||'').toUpperCase(),role=el.getAttribute('role')||'';var pic=tag==='IMG'||tag==='SVG'||tag==='CANVAS';if(pic?!painted(el):!shown(el))continue;var r=vis(el);if(!r||!(pic?frontImage(el,r):front(el,r)))continue;var align=alignOf(el);var dir=getComputedStyle(el).direction;if(tag==='IMG'||tag==='SVG'||tag==='CANVAS'){if(r.width>innerWidth*0.92&&r.height>innerHeight*0.92)continue;if(r.width<12||r.height<12)continue;var src=tag==='IMG'?(el.currentSrc||el.src||''):'';var set=(el.getAttribute&&el.getAttribute('srcset'))||'';if(tag==='SVG'){var im=el.querySelector('image');if(im)src=im.getAttribute('href')||im.getAttribute('xlink:href')||'';}if(el.parentElement&&el.parentElement.tagName==='PICTURE'){var sources=el.parentElement.querySelectorAll('source');for(var si=0;si<sources.length;si++){var ss=sources[si].getAttribute('srcset');if(ss)set=set?set+','+ss:ss;}}push('img',r,src||tag,set,'L');clickHost(el);continue;}if((role==='searchbox'||role==='combobox'||role==='textbox')&&tag!=='INPUT'&&tag!=='TEXTAREA'&&el.querySelector('input,textarea'))continue;if(tag==='INPUT'||tag==='TEXTAREA'||tag==='SELECT'||role==='searchbox'||role==='combobox'||role==='textbox'||el.getAttribute('contenteditable')==='true'){var type=(el.type||'text').toLowerCase();if(type==='hidden')continue;if(r.height>220&&tag!=='TEXTAREA')continue;if(type==='checkbox'||type==='radio'){var cname=el.getAttribute('aria-label')||el.name||type;var ck=pair(el);push('button',r,visual((el.checked?'x ':'')+cname,dir).slice(0,80),'',align,ck[0],ck[1],borderOf(el));continue;}var label=el.getAttribute('aria-label')||el.placeholder||'';if(type==='submit'||type==='button'||type==='image')label=el.value||label||el.name||type;if(!label)label=el.name||type;var value='';if(type!=='submit'&&type!=='button'&&type!=='image'){value=el.value||(el.isContentEditable?el.innerText:'');if(!value&&el.shadowRoot){var inner=el.shadowRoot.querySelector('input,textarea,[contenteditable=true]');if(inner)value=inner.value||inner.innerText||'';}}var kind=(type==='submit'||type==='button'||type==='image')?'button':'field';var face=visual(String(label),dir);var edge=borderOf(el);var fc=pair(el);if(kind==='field'&&fc[1]&&edge&&fc[1]===edge)fc[1]=pageBg;push(kind,r,String(face).replace(/\s+/g,' ').slice(0,80),String(value||'').replace(/\s+/g,' ').slice(0,80),align,fc[0],fc[1],edge);continue;}if(tag==='BUTTON'||role==='button'){var bt=(el.innerText||el.getAttribute('aria-label')||'button').trim().replace(/\s+/g,' ');var bc=pair(el);push('button',r,visual(bt,dir).slice(0,80),'',align,bc[0],bc[1],borderOf(el));continue;}if(tag==='A'||role==='link'){var t=(el.innerText||el.getAttribute('aria-label')||'').trim().replace(/\s+/g,' ');if(!t)t=(el.href||'').replace(/^https?:\/\//,'').slice(0,40);var lc=pair(el);push('link',r,visual(t,dir).slice(0,80),el.href||'',align,lc[0],lc[1],'');}}var clicks=document.querySelectorAll('[onclick],[role=tab],[role=menuitem],[role=option],summary');var foundClick=0;for(var ci=0;ci<clicks.length&&foundClick<40;ci++){var cel=clicks[ci];if(!shown(cel)||cel.closest('a,button,input,textarea,select'))continue;var cr=vis(cel);if(!cr||cr.width<8||cr.height<8)continue;if(cr.width>innerWidth*0.92&&cr.height>innerHeight*0.5)continue;if(!front(cel,cr))continue;var cdir=getComputedStyle(cel).direction;var clabel=(cel.innerText||cel.getAttribute('aria-label')||cel.getAttribute('title')||'open').trim().replace(/\s+/g,' ').slice(0,80);var chref=cel.getAttribute('href')||cel.getAttribute('data-href')||'';var cc=pair(cel);if(chref)push('link',cr,visual(clabel,cdir),chref,alignOf(cel),cc[0],cc[1],'');else push('button',cr,visual(clabel,cdir),'',alignOf(cel),cc[0],cc[1],borderOf(cel));foundClick++;}var hands=document.querySelectorAll('div,span,li,article');var foundHand=0;for(var hi=0;hi<hands.length&&hi<400&&foundHand<24;hi++){var hel=hands[hi];if(!shown(hel)||hel.closest('a,button,input,textarea,select,[onclick],[role=tab],[role=menuitem],[role=option],summary'))continue;if(hel.querySelector('input,textarea,[contenteditable=true],[role=searchbox],[role=combobox],[role=textbox]'))continue;if(getComputedStyle(hel).cursor!=='pointer')continue;var hr2=vis(hel);if(!hr2||hr2.width<16||hr2.height<12)continue;if(hr2.width>innerWidth*0.85||hr2.height>innerHeight*0.45)continue;if(!front(hel,hr2))continue;var hdir=getComputedStyle(hel).direction;var hlabel=(hel.innerText||hel.getAttribute('aria-label')||hel.getAttribute('title')||'open').trim().replace(/\s+/g,' ').slice(0,80);var hhref=hel.getAttribute('href')||hel.getAttribute('data-href')||'';var hc2=pair(hel);if(hhref)push('link',hr2,visual(hlabel,hdir),hhref,alignOf(hel),hc2[0],hc2[1],'');else push('button',hr2,visual(hlabel,hdir),'',alignOf(hel),hc2[0],hc2[1],borderOf(hel));foundHand++;}var boxes=document.querySelectorAll('div,span,a,figure');var found=0;for(var bi=0;bi<boxes.length&&found<6;bi++){var bel=boxes[bi];if(!painted(bel))continue;var bg=getComputedStyle(bel).backgroundImage||'';var bm=bg.match(/url\((['\"]?)(https?:[^)'\"]+|\/[^)'\"]+)\1\)/);if(!bm)continue;var br=vis(bel);if(!br||br.width<48||br.height<24)continue;if(br.width>innerWidth*0.92&&br.height>innerHeight*0.5)continue;if(!frontImage(bel,br))continue;push('img',br,bm[2],'','L');clickHost(bel);found++;}var bars=document.querySelectorAll('header,footer,nav,form,section,div,[role=dialog]');var foundBar=0;for(var bi2=0;bi2<bars.length&&bi2<400&&foundBar<24;bi2++){var bel2=bars[bi2];if(!shown(bel2))continue;var st2=getComputedStyle(bel2);var bgc=hex(st2.backgroundColor);var bdw=parseFloat(st2.borderTopWidth)||parseFloat(st2.borderLeftWidth)||0;var bdc=bdw>=1?(hex(st2.borderTopColor)||hex(st2.borderLeftColor)||''):'';var dialog=bel2.getAttribute('role')==='dialog'||bel2.getAttribute('aria-modal')==='true';if((!bgc||bgc===pageBg)&&!bdc&&!dialog)continue;if(dialog&&!bdc)bdc='dadce0';var brr=vis(bel2);if(!brr||brr.width<32||brr.height<12||brr.height>innerHeight*0.92)continue;if(brr.width>innerWidth*0.98&&brr.height>innerHeight*0.72)continue;if(!front(bel2,brr))continue;push('bar',brr,'','','L',hex(st2.color),bgc,bdc);foundBar++;}function clickHost(el){var host=el.closest('a[href],button,[role=button],[role=link]');if(!host||host===el)return;var hr=vis(host);if(!hr||!front(host,hr))return;var htag=host.tagName,hrole=host.getAttribute('role')||'',hdir=getComputedStyle(host).direction,hc=pair(host);if(htag==='BUTTON'||hrole==='button'){var bt=(host.innerText||host.getAttribute('aria-label')||'button').trim().replace(/\s+/g,' ');push('button',hr,visual(bt,hdir).slice(0,80),'',alignOf(host),hc[0],hc[1],borderOf(host));return;}var t=(host.innerText||host.getAttribute('aria-label')||'').trim().replace(/\s+/g,' ');var href=host.href||host.getAttribute('href')||'';if(!t)t=href.replace(/^https?:\/\//,'').slice(0,40);push('link',hr,visual(t,hdir).slice(0,80),href,alignOf(host),hc[0],hc[1],'');}function visual(s,dir){return s;}function inkGone(el){var c=getComputedStyle(el).color;if(!c||c==='transparent')return true;var m=String(c).match(/rgba?\(\s*\d+[,\s]+\d+[,\s]+\d+(?:[,\s\/]+([0-9.]+))?\s*\)/);return !!(m&&m[1]!=null&&Number(m[1])<0.05);}var walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);var n;while((n=walker.nextNode())){var raw=n.textContent;if(!raw||!raw.trim())continue;var p=n.parentElement;if(!p)continue;var pt=p.tagName;if(pt==='SCRIPT'||pt==='STYLE'||pt==='NOSCRIPT')continue;if(p.closest('a,button,input,textarea,select,script,style,svg,[role=button],[role=link]'))continue;if(!shown(p)||inkGone(p))continue;var tdir=getComputedStyle(p).direction;var talign=alignOf(p);var parts=linesOf(n,raw);if(!parts.length){var range=document.createRange();range.selectNodeContents(n);var box=range.getBoundingClientRect();var whole=raw.replace(/\s+/g,' ').trim().slice(0,160);if(whole&&front(p,box))push('text',box,visual(whole,tdir),'',talign,pair(p)[0],'','');continue;}for(var pi=0;pi<parts.length;pi++){var part=parts[pi];var bit=part.text.replace(/\s+/g,' ').trim();if(!bit||!front(p,part.box))continue;push('text',part.box,visual(bit,tdir).slice(0,160),'',talign,pair(p)[0],'','');}}return lines.join('\n');})()"#;

const TRAFFIC: &str = r#"(function(){var nav=performance.getEntriesByType('navigation')[0];var page=0;if(nav)page=nav.transferSize||nav.encodedBodySize||nav.decodedBodySize||0;var net=page;var list=performance.getEntriesByType('resource');for(var i=0;i<list.length;i++){net+=list[i].transferSize||list[i].encodedBodySize||0;}return Math.round(page)+'\t'+Math.round(net);})()"#;

fn page_list(port: u16) -> Result<String, String> {
    let mut last = String::from("page list missing");
    for path in ["/json/list", "/json"] {
        match http_get(port, path) {
            Ok(body) if page_socket(&body).is_some() => return Ok(body),
            Ok(body) => last = body.chars().take(160).collect(),
            Err(err) => last = err,
        }
    }
    Err(last)
}

fn page_socket(body: &str) -> Option<String> {
    let mut rest = body;
    while let Some(at) = rest.find("\"type\":\"page\"").or_else(|| rest.find("\"type\": \"page\"")) {
        let start = rest[..at].rfind('{').unwrap_or(0);
        if let Some(url) = json_field(&rest[start..], "webSocketDebuggerUrl") {
            if url.contains("/devtools/page/") {
                return Some(url);
            }
        }
        rest = &rest[at + 8..];
    }
    None
}

fn cdp_error(text: &str) -> Option<String> {
    let at = text.find("\"error\":{")?;
    if text.find("\"result\"").is_some_and(|result| result < at) {
        return None;
    }
    json_field(&text[at..], "message")
}

fn wheel_delta(delta: i32) -> i32 {
    if delta < 0 { -64 } else { 64 }
}

fn focus_js(x: i32, y: i32) -> String {
    format!(
        "(function(){{var x={x};var y={y};{body}}})()",
        body = FOCUS_BODY
    )
}

fn field_js(x: i32, y: i32, text: &str, erase: bool) -> String {
    format!(
        "(function(){{var text={};var x={x};var y={y};var erase={erase};{body}}})()",
        json_str(text),
        erase = if erase { "true" } else { "false" },
        body = FIELD_BODY
    )
}

fn address_line(url: &str, fg: Option<u32>, bg: Option<u32>) -> String {
    let Some((fg, bg)) = fg.zip(bg) else {
        return format!("{DIM}{url}");
    };
    format!(
        "\x1b[38;2;{};{};{};48;2;{};{};{}m{url}",
        (fg >> 16) & 255,
        (fg >> 8) & 255,
        fg & 255,
        (bg >> 16) & 255,
        (bg >> 8) & 255,
        bg & 255
    )
}

fn split_point(text: &str) -> Option<(i32, i32)> {
    let (left, right) = text.split_once(',')?;
    Some((left.trim().parse().ok()?, right.trim().parse().ok()?))
}

const FOCUS_BODY: &str = r#"function pick(node){if(!node||!node.tagName)return null;var type=(node.type||'').toLowerCase();if(type==='hidden'||type==='checkbox'||type==='radio'||type==='button'||type==='submit'||type==='image'||type==='file'||type==='reset')return null;var tag=node.tagName;var role=(node.getAttribute&&node.getAttribute('role'))||'';if(tag==='INPUT'||tag==='TEXTAREA'||tag==='SELECT'||node.isContentEditable||role==='searchbox'||role==='textbox'||role==='combobox')return node;var root=node.shadowRoot;if(root){var inner=root.querySelector('input,textarea,select,[contenteditable=true]');if(inner)return pick(inner)||inner;}return null;}var stack=document.elementsFromPoint(x,y)||[];var el=null;for(var i=0;i<stack.length;i++){el=pick(stack[i]);if(el)break;if(stack[i].querySelector){el=pick(stack[i].querySelector('input,textarea,select,[contenteditable="true"]'));if(el)break;}var id=stack[i].getAttribute&&stack[i].getAttribute('for');if(id){el=pick(document.getElementById(id));if(el)break;}}if(!el)return 'none';el.focus();try{var len=String(el.value||'').length;el.setSelectionRange(len,len);}catch(e){}var r=el.getBoundingClientRect();var cx=Math.round(r.left+Math.max(4,Math.min(12,r.width/2)));var cy=Math.round(r.top+r.height/2);var top=document.elementFromPoint(cx,cy);if(top===el||(el.contains&&el.contains(top)))return cx+','+cy;return 'focused';"#;

const FIELD_BODY: &str = r#"function pick(node){if(!node||!node.tagName)return null;var type=(node.type||'').toLowerCase();if(type==='hidden'||type==='checkbox'||type==='radio'||type==='button'||type==='submit'||type==='image'||type==='file'||type==='reset')return null;var tag=node.tagName;var role=(node.getAttribute&&node.getAttribute('role'))||'';if(tag==='INPUT'||tag==='TEXTAREA'||tag==='SELECT'||node.isContentEditable||role==='searchbox'||role==='textbox'||role==='combobox')return node;var root=node.shadowRoot;if(root){var inner=root.querySelector('input,textarea,select,[contenteditable=true]');if(inner)return pick(inner)||inner;}return null;}function find(){var live=pick(document.activeElement);if(live)return live;var stack=document.elementsFromPoint(x,y)||[];for(var i=0;i<stack.length;i++){var hit=pick(stack[i]);if(hit)return hit;if(stack[i].querySelector){hit=pick(stack[i].querySelector('input,textarea,select,[contenteditable="true"]'));if(hit)return hit;}var id=stack[i].getAttribute&&stack[i].getAttribute('for');if(id){hit=pick(document.getElementById(id));if(hit)return hit;}}return null;}var el=find();if(!el)return 'none';el.focus();if(el.tagName!=='INPUT'&&el.tagName!=='TEXTAREA'&&el.tagName!=='SELECT'&&!el.isContentEditable)return 'none';if(el.isContentEditable){if(erase){try{document.execCommand('delete',false,null);}catch(e){}return 'ok';}var before=el.innerText||'';var wrote=false;try{wrote=document.execCommand('insertText',false,text);}catch(e){}if(!wrote||(el.innerText||'')===before){var node=document.createTextNode(text);var sel=window.getSelection();if(sel&&sel.rangeCount&&el.contains(sel.anchorNode)){var range=sel.getRangeAt(0);range.deleteContents();range.insertNode(node);range.setStartAfter(node);range.collapse(true);sel.removeAllRanges();sel.addRange(range);}else{el.appendChild(node);}}try{el.dispatchEvent(new InputEvent('input',{bubbles:true,data:text}));}catch(e){}return 'ok';}if(el.tagName==='SELECT')return 'none';var value=String(el.value||'');var start=el.selectionStart;var end=el.selectionEnd;if(start==null||end==null){start=value.length;end=value.length;}if(erase){if(start===end&&start>0)start=start-1;el.value=value.slice(0,start)+value.slice(end);try{el.setSelectionRange(start,start);}catch(e){}}else{el.value=value.slice(0,start)+text+value.slice(end);var caret=start+text.length;try{el.setSelectionRange(caret,caret);}catch(e){}}try{el.dispatchEvent(new Event('input',{bubbles:true}));}catch(e){}return 'ok';"#;

fn wheel_js(dy: i32) -> String {
    format!(
        "(function(){{var dy={dy};function snap(n){{return Math.round(n/16)*16;}}function scroller(el){{while(el&&el!==document.body&&el!==document.documentElement){{var s=getComputedStyle(el);var oy=s.overflowY;if((oy==='auto'||oy==='scroll'||oy==='overlay')&&el.scrollHeight>el.clientHeight+8&&el.clientHeight>innerHeight*0.55)return el;el=el.parentElement;}}return document.scrollingElement||document.documentElement;}}var hit=document.elementFromPoint(Math.floor(innerWidth/2),Math.floor(innerHeight/2));var el=scroller(hit)||document.scrollingElement||document.documentElement;var before=el.scrollTop;el.scrollTop=snap(before+dy);if(el.scrollTop===before){{var root=document.scrollingElement||document.documentElement;if(root!==el)root.scrollTop=snap(root.scrollTop+dy);}}return 'ok';}})()"
    )
}

fn plain_agent(ua: &str) -> String {
    let cleaned = ua.replace("Headless", "");
    if cleaned.contains("Mozilla/") && (cleaned.contains("Chrome/") || cleaned.contains("Edg/")) {
        cleaned
    } else {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0".into()
    }
}

fn script_blocked(err: &str) -> bool {
    let low = err.to_ascii_lowercase();
    low.contains("script") && low.contains("disabled")
}

fn accept_language(locale: &str) -> String {
    let primary = locale.split('-').next().unwrap_or(locale);
    if primary.eq_ignore_ascii_case(locale) {
        format!("{locale},en;q=0.5")
    } else if primary.eq_ignore_ascii_case("en") {
        format!("{locale},{primary};q=0.9")
    } else {
        format!("{locale},{primary};q=0.9,en;q=0.5")
    }
}

fn prefer_google_lang(url: &str, locale: &str) -> String {
    let lang = locale.split(['-', '_']).next().unwrap_or("").trim();
    if lang.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
        return url.to_string();
    }
    let Some(host) = url_host(url) else {
        return url.to_string();
    };
    if !google_host(&host) {
        return url.to_string();
    }
    set_query(url, "hl", lang)
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn google_host(host: &str) -> bool {
    let bare = host.strip_prefix("www.").unwrap_or(host);
    bare == "google.com" || bare.starts_with("google.") || bare.ends_with(".google.com")
}

fn set_query(url: &str, key: &str, value: &str) -> String {
    let (head, hash) = match url.split_once('#') {
        Some((body, fragment)) => (body, format!("#{fragment}")),
        None => (url, String::new()),
    };
    let Some((path, query)) = head.split_once('?') else {
        return format!("{head}?{key}={value}{hash}");
    };
    let mut parts = Vec::new();
    let mut replaced = false;
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let name = part.split('=').next().unwrap_or("");
        if name == key {
            if !replaced {
                parts.push(format!("{key}={value}"));
                replaced = true;
            }
        } else {
            parts.push(part.to_string());
        }
    }
    if !replaced {
        parts.push(format!("{key}={value}"));
    }
    format!("{path}?{}{hash}", parts.join("&"))
}

fn fit_view(cols: usize, rows: usize) -> (usize, usize, usize) {
    let cols = cols.clamp(40, 240);
    let rows = rows.clamp(8, 60);
    let view_rows = rows.saturating_sub(1).max(6);
    (cols, rows, view_rows)
}

fn response_id(text: &str, id: u64) -> bool {
    let head: String = text.chars().take(64).collect();
    head.contains(&format!("\"id\":{id},")) || head.contains(&format!("\"id\":{id}}}"))
}

fn parse_nodes(text: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let kind = parts.next().unwrap_or("").to_string();
        let x = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let y = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let w = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let h = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let text = parts.next().unwrap_or("").to_string();
        let extra = parts.next().unwrap_or("").to_string();
        let align = parts.next().unwrap_or("L").chars().next().unwrap_or('L');
        let fg = parts.next().unwrap_or("").to_string();
        let bg = parts.next().unwrap_or("").to_string();
        let bd = parts.next().unwrap_or("").to_string();
        if kind.is_empty() || w <= 1 || h <= 1 {
            continue;
        }
        nodes.push(Node {
            kind,
            x,
            y,
            w,
            h,
            text,
            extra,
            align,
            fg,
            bg,
            bd,
        });
    }
    nodes
}

fn compose(nodes: &[Node], shots: &[Option<String>], cols: usize, rows: usize) -> WebView {
    let cols = cols.max(8);
    let rows = rows.max(4);
    let mut grid = vec![vec![Cell { ch: ' ', ink: "", fg: None, bg: None }; cols]; rows];
    let mut hits = Vec::new();
    let mut blots = Vec::new();
    for node in nodes {
        if node.kind != "wash" && node.kind != "bar" {
            continue;
        }
        let Some((col, row, wide, high)) = cell_box(node, cols, rows) else {
            continue;
        };
        if node.kind == "bar" && (bar_is_field_sliver(node, nodes) || bar_is_field_ring(node, nodes))
        {
            continue;
        }
        fill_box(&mut grid, col, row, wide, high, rgb(&node.bg), node.kind == "bar", &[]);
        if let Some(color) = rgb(&node.bd) {
            draw_frame(&mut grid, col, row, wide, high, color, &[]);
        }
    }
    for node in nodes {
        if node.kind != "text" {
            continue;
        }
        let Some((col, row, wide, _)) = cell_box(node, cols, rows) else {
            continue;
        };
        place_text(
            &mut grid,
            col,
            row,
            wide,
            &node.text,
            node.align,
            TEXT,
            rgb(&node.fg),
        );
    }
    for (index, node) in nodes.iter().enumerate() {
        if node.kind != "img" {
            continue;
        }
        let Some((col, row, wide, high)) = cell_box(node, cols, rows) else {
            continue;
        };
        let Some(Some(picture)) = shots.get(index) else {
            continue;
        };
        let lines: Vec<String> = picture
            .lines()
            .filter(|line| !line.is_empty())
            .take(high)
            .map(str::to_string)
            .collect();
        if lines.is_empty() {
            continue;
        }
        let glyphs = visible_len(&lines[0]).clamp(1, wide);
        blots.push(Blot {
            col,
            row,
            cols: glyphs,
            lines,
        });
    }
    paint_controls(nodes, &mut grid, &mut hits, cols, rows);
    let mut lines = Vec::with_capacity(rows);
    for y in 0..rows {
        let mut out = String::new();
        let mut prev = Cell { ch: ' ', ink: "\n", fg: None, bg: None };
        let mut col = 0usize;
        let rtl_row = grid[y].iter().any(|cell| strong_rtl(cell.ch));
        let mut in_rtl = false;
        if rtl_row {
            out.push('\u{2066}');
        }
        while col < cols {
            if let Some(blot) = blots
                .iter()
                .find(|blot| blot.col == col && y >= blot.row && y < blot.row + blot.lines.len())
            {
                if in_rtl {
                    out.push('\u{2069}');
                    in_rtl = false;
                }
                out.push_str("\x1b[0m");
                prev = Cell { ch: ' ', ink: "\n", fg: None, bg: None };
                out.push_str(&blot.lines[y - blot.row]);
                col += blot.cols.max(1);
                continue;
            }
            let cell = grid[y][col];
            let want = rtl_row && rtl_at(&grid[y], col);
            if in_rtl && (!want || style_changes(&cell, &prev)) {
                out.push('\u{2069}');
                in_rtl = false;
            }
            paint_cell(&mut out, &cell, &prev);
            if want && !in_rtl {
                out.push('\u{2067}');
                in_rtl = true;
            }
            prev = cell;
            out.push(cell.ch);
            col += 1;
        }
        if in_rtl {
            out.push('\u{2069}');
        }
        if rtl_row {
            out.push('\u{2069}');
        }
        let washed = grid[y].iter().any(|cell| cell.bg.is_some());
        lines.push(if washed { out } else { out.trim_end().to_string() });
    }
    WebView {
        url: String::new(),
        lines,
        hits,
    }
}

const OWN_FIELD: u8 = 1;
const OWN_CHROME: u8 = 2;

fn paint_controls(nodes: &[Node], grid: &mut [Vec<Cell>], hits: &mut Vec<WebHit>, cols: usize, rows: usize) {
    let mut owned = vec![vec![0u8; cols]; rows];
    let mut icons = Vec::new();
    for node in nodes {
        let Some((col, row, wide, high)) = cell_box(node, cols, rows) else {
            continue;
        };
        match node.kind.as_str() {
            "button" if format!("[{}]", node.text).chars().count() > wide => {
                icons.push((node, col, row, wide, high));
            }
            "link" | "field" | "button" => {
                paint_fitted(node, col, row, wide, high, grid, &mut owned, hits);
            }
            _ => {}
        }
    }
    for (node, col, row, wide, high) in icons {
        paint_icon(node, col, row, wide, high, grid, &mut owned, hits);
    }
}

fn paint_fitted(
    node: &Node,
    col: usize,
    row: usize,
    wide: usize,
    high: usize,
    grid: &mut [Vec<Cell>],
    owned: &mut [Vec<u8>],
    hits: &mut Vec<WebHit>,
) {
    let (kind, code, label) = match node.kind.as_str() {
        "link" => (
            KIND_LINK,
            OWN_CHROME,
            if node.text.is_empty() { node.extra.clone() } else { node.text.clone() },
        ),
        "field" => (KIND_FIELD, OWN_FIELD, String::new()),
        "button" => (KIND_BUTTON, OWN_CHROME, format!("[{}]", node.text)),
        _ => return,
    };
    let fill_high = if kind == KIND_LINK { 1 } else { high };
    if kind == KIND_FIELD {
        let edge = rgb(&node.bd).unwrap_or(0xdadce0);
        let paper = field_paper(rgb(&node.bg), rgb(&node.bd));
        fill_box(grid, col, row, wide, high, Some(paper), true, owned);
        let shown = if node.extra.is_empty() { node.text.clone() } else { node.extra.clone() };
        if high >= 3 && wide >= 4 {
            draw_frame(grid, col, row, wide, high, edge, owned);
            let (x, room) = free_run(owned, col + 2, row + high / 2, wide.saturating_sub(4).max(1))
                .unwrap_or((col + 2, wide.saturating_sub(4).max(1)));
            write_span(grid, x, row + high / 2, room, &fit_text(&shown, room), 'L', FIELD, rgb(&node.fg), true);
        } else {
            let (x, room) = free_run(owned, col, row, wide).unwrap_or((col, wide));
            let inner = room.saturating_sub(2).max(1);
            let body = fit_text(&shown, inner);
            let gap = inner.saturating_sub(body.chars().count());
            let drawn = format!("[{body}{}]", " ".repeat(gap));
            write_span(grid, x, row, room.min(drawn.chars().count().max(1)), &drawn, 'L', FIELD, rgb(&node.fg), true);
            if high >= 2 {
                draw_frame(grid, col, row + high - 1, wide, 1, edge, owned);
            }
        }
    } else {
        let bg = rgb(&node.bg);
        fill_box(grid, col, row, wide, fill_high, bg, kind == KIND_BUTTON, owned);
        if kind == KIND_BUTTON
            && let Some(color) = rgb(&node.bd)
        {
            draw_frame(grid, col, row, wide, high, color, owned);
        }
        let label_row = if kind == KIND_BUTTON { row + high / 2 } else { row };
        if let Some((x, room)) = free_run(owned, col, label_row, wide) {
            write_span(grid, x, label_row, room, &label, node.align, LINK, rgb(&node.fg), true);
        }
    }
    claim(owned, col, row, wide, fill_high, code, false);
    hits.push(WebHit {
        x: col,
        y: row,
        w: wide,
        h: high,
        kind,
        href: if kind == KIND_LINK { node.extra.clone() } else { String::new() },
        label: if kind == KIND_BUTTON { node.text.clone() } else if kind == KIND_LINK { label } else { node.text.clone() },
        px: if kind == KIND_FIELD { node.x + 8 } else { node.x + node.w / 2 },
        py: node.y + node.h / 2,
    });
}

fn paint_icon(
    node: &Node,
    col: usize,
    row: usize,
    wide: usize,
    high: usize,
    grid: &mut [Vec<Cell>],
    owned: &mut [Vec<u8>],
    hits: &mut Vec<WebHit>,
) {
    let mark = if wide >= 3 { "[•]" } else { "•" };
    let need = mark.chars().count();
    let mid = row + high / 2;
    if let Some(x) = icon_slot(owned, col, mid, wide, need) {
        write_span(grid, x, mid, need, mark, 'L', LINK, rgb(&node.fg), true);
        claim(owned, x, mid, need, 1, OWN_CHROME, true);
    }
    hits.push(WebHit {
        x: col,
        y: row,
        w: wide,
        h: high,
        kind: KIND_BUTTON,
        href: String::new(),
        label: node.text.clone(),
        px: node.x + node.w / 2,
        py: node.y + node.h / 2,
    });
}

fn field_paper(bg: Option<u32>, edge: Option<u32>) -> u32 {
    let Some(fill) = bg else {
        return 0xffffff;
    };
    if edge == Some(fill) || (edge.is_some() && saturated(fill)) {
        return 0xffffff;
    }
    fill
}

fn saturated(color: u32) -> bool {
    let r = (color >> 16) & 255;
    let g = (color >> 8) & 255;
    let b = color & 255;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    max - min > 48 && max > 80
}

/// A focus ring is a bar a few pixels larger than the field. The field paints
/// its own paper and border, so the ring must not fill the box.
fn bar_is_field_ring(bar: &Node, nodes: &[Node]) -> bool {
    nodes.iter().any(|other| {
        other.kind == "field"
            && (bar.x - other.x).abs() <= 32
            && (bar.y - other.y).abs() <= 32
            && (bar.w - other.w).abs() <= 120
            && (bar.h - other.h).abs() <= 64
    })
}

fn bar_is_field_sliver(bar: &Node, nodes: &[Node]) -> bool {
    for other in nodes {
        if other.kind != "field" {
            continue;
        }
        let overlap_y = overlap_px(bar.y, bar.h, other.y, other.h);
        let band = bar.h.min(other.h).max(1);
        if overlap_y * 100 < band * 70 {
            continue;
        }
        let overlap_x = overlap_px(bar.x, bar.w, other.x, other.w);
        let gap = if overlap_x > 0 {
            0
        } else {
            let bar_right = bar.x.saturating_add(bar.w);
            let field_right = other.x.saturating_add(other.w);
            if bar_right <= other.x {
                other.x - bar_right
            } else if field_right <= bar.x {
                bar.x - field_right
            } else {
                0
            }
        };
        if gap > 16 || bar.h > other.h + 28 {
            continue;
        }
        let leftover = if overlap_x > 0 { bar.w.saturating_sub(overlap_x) } else { bar.w };
        if leftover <= 80 {
            return true;
        }
    }
    false
}

fn overlap_px(origin: i32, len: i32, other: i32, other_len: i32) -> i32 {
    let end = origin.saturating_add(len);
    let other_end = other.saturating_add(other_len);
    end.min(other_end).saturating_sub(origin.max(other)).max(0)
}

fn taken_cell(owned: &[Vec<u8>], x: usize, y: usize) -> bool {
    owned.get(y).and_then(|line| line.get(x)).copied().unwrap_or(0) != 0
}

fn claim(
    owned: &mut [Vec<u8>],
    col: usize,
    row: usize,
    wide: usize,
    high: usize,
    code: u8,
    steal_field: bool,
) {
    for y in row..row.saturating_add(high) {
        let Some(line) = owned.get_mut(y) else {
            continue;
        };
        let end = (col + wide).min(line.len());
        for cell in &mut line[col..end] {
            if *cell == 0 || (steal_field && *cell == OWN_FIELD) {
                *cell = code;
            }
        }
    }
}

fn free_run(owned: &[Vec<u8>], col: usize, row: usize, wide: usize) -> Option<(usize, usize)> {
    let line = owned.get(row)?;
    let end = (col + wide).min(line.len());
    let mut best: Option<(usize, usize)> = None;
    let mut start = None;
    for x in col..end {
        if line[x] == 0 {
            if start.is_none() {
                start = Some(x);
            }
        } else if let Some(from) = start.take() {
            let len = x - from;
            if best.is_none_or(|(_, had)| len > had) {
                best = Some((from, len));
            }
        }
    }
    if let Some(from) = start {
        let len = end - from;
        if best.is_none_or(|(_, had)| len > had) {
            best = Some((from, len));
        }
    }
    best.filter(|(_, len)| *len > 0)
}

fn icon_slot(owned: &[Vec<u8>], col: usize, row: usize, wide: usize, need: usize) -> Option<usize> {
    if need == 0 || wide < need {
        return None;
    }
    let center = col + (wide - need) / 2;
    if icon_fits(owned, center, row, need) {
        return Some(center);
    }
    let end = col + wide;
    let mut x = col;
    while x + need <= end {
        if icon_fits(owned, x, row, need) {
            return Some(x);
        }
        x += 1;
    }
    None
}

fn icon_fits(owned: &[Vec<u8>], x: usize, row: usize, need: usize) -> bool {
    let Some(line) = owned.get(row) else {
        return false;
    };
    (x..x + need).all(|index| line.get(index).copied().unwrap_or(OWN_CHROME) != OWN_CHROME)
}

fn cell_box(node: &Node, cols: usize, rows: usize) -> Option<(usize, usize, usize, usize)> {
    if node.kind == "text" && node.h > 48 {
        return None;
    }
    let view_w = (cols as i32).saturating_mul(8);
    let view_h = (rows as i32).saturating_mul(16);
    let x0 = node.x.max(0);
    let y0 = node.y.max(0);
    let x1 = node.x.saturating_add(node.w).min(view_w);
    let y1 = node.y.saturating_add(node.h).min(view_h);
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return None;
    }
    let col = (x0 as usize) / 8;
    let row = (y0 as usize) / 16;
    if col >= cols || row >= rows {
        return None;
    }
    let mut wide = ((x1 as usize + 7) / 8)
        .saturating_sub(col)
        .clamp(1, cols - col);
    let mut high = ((y1 as usize + 15) / 16)
        .saturating_sub(row)
        .clamp(1, rows - row);
    if node.kind == "img" {
        (wide, high) = crate::draw::fit_cells(wide, high, 64, 12);
        wide = wide.min(cols - col).max(1);
        high = high.min(rows - row).max(1);
    }
    Some((col, row, wide, high))
}

fn rgb(hex: &str) -> Option<u32> {
    let hex = hex.trim();
    if hex.len() != 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

fn fill_box(
    grid: &mut [Vec<Cell>],
    col: usize,
    row: usize,
    wide: usize,
    high: usize,
    bg: Option<u32>,
    clear: bool,
    owned: &[Vec<u8>],
) {
    if bg.is_none() && !clear {
        return;
    }
    for y in row..row.saturating_add(high) {
        let Some(line) = grid.get_mut(y) else {
            continue;
        };
        let end = (col + wide).min(line.len());
        for (offset, cell) in line[col..end].iter_mut().enumerate() {
            if taken_cell(owned, col + offset, y) {
                continue;
            }
            if let Some(bg) = bg {
                cell.bg = Some(bg);
            }
            if clear {
                cell.ch = ' ';
                cell.fg = None;
                cell.ink = "";
            }
        }
    }
}

fn draw_frame(
    grid: &mut [Vec<Cell>],
    col: usize,
    row: usize,
    wide: usize,
    high: usize,
    color: u32,
    owned: &[Vec<u8>],
) {
    if wide < 2 || high < 1 {
        return;
    }
    let paint = |grid: &mut [Vec<Cell>], x: usize, y: usize, ch: char| {
        if taken_cell(owned, x, y) {
            return;
        }
        let Some(line) = grid.get_mut(y) else {
            return;
        };
        let Some(cell) = line.get_mut(x) else {
            return;
        };
        let bg = cell.bg;
        *cell = Cell { ch, ink: "", fg: Some(color), bg };
    };
    if high == 1 {
        for x in col..col + wide {
            paint(grid, x, row, '─');
        }
        return;
    }
    paint(grid, col, row, '┌');
    paint(grid, col + wide - 1, row, '┐');
    paint(grid, col, row + high - 1, '└');
    paint(grid, col + wide - 1, row + high - 1, '┘');
    for x in col + 1..col + wide - 1 {
        paint(grid, x, row, '─');
        paint(grid, x, row + high - 1, '─');
    }
    for y in row + 1..row + high - 1 {
        paint(grid, col, y, '│');
        paint(grid, col + wide - 1, y, '│');
    }
}

fn paint_cell(out: &mut String, cell: &Cell, prev: &Cell) {
    if cell.bg != prev.bg {
        if let Some(bg) = cell.bg {
            out.push_str(&format!(
                "\x1b[48;2;{};{};{}m",
                (bg >> 16) & 255,
                (bg >> 8) & 255,
                bg & 255
            ));
        } else {
            out.push_str("\x1b[49m");
        }
    }
    if cell.fg != prev.fg || (cell.fg.is_none() && cell.ink != prev.ink) {
        if let Some(fg) = cell.fg {
            out.push_str(&format!(
                "\x1b[38;2;{};{};{}m",
                (fg >> 16) & 255,
                (fg >> 8) & 255,
                fg & 255
            ));
        } else if !cell.ink.is_empty() {
            out.push_str(cell.ink);
        } else if prev.fg.is_some() || (!prev.ink.is_empty() && prev.ink != "\n") {
            out.push_str("\x1b[39m");
        }
    }
}

fn place_text(
    grid: &mut [Vec<Cell>],
    col: usize,
    mut row: usize,
    wide: usize,
    text: &str,
    align: char,
    ink: &'static str,
    fg: Option<u32>,
) {
    let count = text
        .chars()
        .filter(|ch| *ch != '\t' && *ch != '\n')
        .count();
    if count == 0 || wide == 0 {
        return;
    }
    let need = count.min(wide).max(1);
    if taken(grid, col, row, wide) * 2 >= need {
        if row + 1 < grid.len() && taken(grid, col, row + 1, wide) == 0 {
            row += 1;
        } else {
            return;
        }
    }
    write_span(grid, col, row, wide, text, align, ink, fg, false);
}

fn taken(grid: &[Vec<Cell>], col: usize, row: usize, wide: usize) -> usize {
    let Some(line) = grid.get(row) else {
        return 0;
    };
    let end = (col + wide).min(line.len());
    line[col..end].iter().filter(|cell| cell.ch != ' ').count()
}

fn style_changes(cell: &Cell, prev: &Cell) -> bool {
    cell.bg != prev.bg || cell.fg != prev.fg || (cell.fg.is_none() && cell.ink != prev.ink)
}

fn strong_rtl(ch: char) -> bool {
    matches!(
        ch,
        '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'
    )
}

fn strong_ltr(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ('\u{00C0}'..='\u{024F}').contains(&ch)
}

fn rtl_at(line: &[Cell], index: usize) -> bool {
    let ch = line[index].ch;
    if strong_rtl(ch) {
        return true;
    }
    if strong_ltr(ch) {
        return false;
    }
    let prev = line[..index].iter().rev().find_map(|cell| {
        if strong_rtl(cell.ch) {
            Some(true)
        } else if strong_ltr(cell.ch) {
            Some(false)
        } else {
            None
        }
    });
    let next = line[index + 1..].iter().find_map(|cell| {
        if strong_rtl(cell.ch) {
            Some(true)
        } else if strong_ltr(cell.ch) {
            Some(false)
        } else {
            None
        }
    });
    prev == Some(true) && next == Some(true)
}

fn fit_text(text: &str, width: usize) -> String {
    let chars: Vec<char> = text
        .chars()
        .filter(|ch| *ch != '\t' && *ch != '\n')
        .collect();
    let end = boundary_end(&chars, width);
    chars.into_iter().take(end).collect()
}

fn boundary_end(chars: &[char], width: usize) -> usize {
    if width == 0 {
        return 0;
    }
    if chars.len() <= width {
        return chars.len();
    }
    let floor = width.saturating_sub(18).max(width / 2);
    for index in (floor..width).rev() {
        let ch = chars[index];
        if ch.is_whitespace() {
            return index.max(1);
        }
        if matches!(ch, '/' | '?' | '&' | '=' | '-' | ',' | '.') {
            return (index + 1).min(width);
        }
    }
    width
}

fn write_span(
    grid: &mut [Vec<Cell>],
    col: usize,
    row: usize,
    width: usize,
    text: &str,
    align: char,
    ink: &'static str,
    fg: Option<u32>,
    replace: bool,
) {
    let Some(line) = grid.get_mut(row) else {
        return;
    };
    let room = line.len().saturating_sub(col);
    let width = width.min(room);
    if width == 0 {
        return;
    }
    let chars: Vec<char> = text.chars().filter(|ch| *ch != '\t' && *ch != '\n').collect();
    let count = if chars.len() > width && align != 'R' && align != 'C' {
        boundary_end(&chars, width)
    } else {
        chars.len().min(width)
    };
    let skip = if align == 'R' { chars.len() - count } else { 0 };
    let start = match align {
        'R' => col + width - count,
        'C' => col + (width - count) / 2,
        _ => col,
    };
    for (index, ch) in chars.into_iter().skip(skip).take(count).enumerate() {
        let x = start + index;
        if x >= col + width || x >= line.len() {
            break;
        }
        if !replace && line[x].ch != ' ' {
            continue;
        }
        let bg = line[x].bg;
        line[x] = Cell {
            ch,
            ink: if fg.is_some() { "" } else { ink },
            fg,
            bg,
        };
    }
}

fn visible_len(line: &str) -> usize {
    let mut count = 0usize;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        count += 1;
    }
    count
}

fn picture_of(bytes: &[u8], cell_w: usize, cell_h: usize) -> Option<String> {
    let image = image::load_from_memory(bytes).ok()?;
    if image.width() < 8 || image.height() < 8 || washed(&image) {
        return None;
    }
    let picture = crate::draw::portrait_fill(&image, cell_w, cell_h);
    crate::draw::has_blocks(&picture).then_some(picture)
}

/// A failed clip of a transparent SVG is an almost white tile. A wordmark
/// still has colored letters, so it stays.
fn washed(image: &image::DynamicImage) -> bool {
    let small = image
        .resize(16, 8, image::imageops::FilterType::Nearest)
        .to_rgb8();
    let total = small.pixels().len();
    if total == 0 {
        return true;
    }
    let mut pale = 0usize;
    for pixel in small.pixels() {
        if pixel[0] > 245 && pixel[1] > 245 && pixel[2] > 245 {
            pale += 1;
        }
    }
    pale * 100 >= total * 97
}

fn png_bytes(data_url: &str) -> Option<Vec<u8>> {
    let rest = data_url.trim().strip_prefix("data:image/png;base64,")?;
    b64_decode(rest)
}

fn shot_key(node: &Node) -> String {
    if node.text.starts_with("http")
        || node.text.starts_with("data:")
        || node.text.starts_with('/')
    {
        return node.text.clone();
    }
    // Vertical position is left out. A scroll must reuse the picture already
    // taken for this icon instead of dropping it.
    format!("{}:{}:{}x{}", node.text, node.x / 8, node.w, node.h)
}

fn browser_path() -> Option<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(root) = std::env::var("ProgramFiles(x86)") {
        paths.push(PathBuf::from(root).join("Microsoft/Edge/Application/msedge.exe"));
    }
    if let Ok(root) = std::env::var("ProgramFiles") {
        paths.push(PathBuf::from(&root).join("Microsoft/Edge/Application/msedge.exe"));
        paths.push(PathBuf::from(&root).join("Google/Chrome/Application/chrome.exe"));
    }
    if let Ok(root) = std::env::var("LOCALAPPDATA") {
        paths.push(PathBuf::from(&root).join("Google/Chrome/Application/chrome.exe"));
        paths.push(PathBuf::from(root).join("Microsoft/Edge/Application/msedge.exe"));
    }
    paths.into_iter().find(|path| path.is_file())
}

fn split_ws(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("ws://")?;
    let (hostport, path) = rest.split_once('/')?;
    let (host, port) = hostport.split_once(':')?;
    Some((host.to_string(), port.parse().ok()?, format!("/{path}")))
}

fn profile_name(dir: &std::path::Path) -> Option<String> {
    let name = dir.file_name()?.to_str()?;
    let rest = name.strip_prefix("granttap-edge-")?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(name.to_string())
}

fn end_profile(dir: &std::path::Path) {
    let Some(name) = profile_name(dir) else {
        return;
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let script = format!(
            "Get-CimInstance Win32_Process -Filter \"Name = 'msedge.exe'\" | Where-Object {{ $_.CommandLine -and $_.CommandLine.Contains('{name}') }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
        );
        let _ = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = name;
    }
}

#[cfg(test)]
fn port_busy(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

fn brief_net(err: &str) -> String {
    let err = err.trim();
    if err.contains("did not properly respond")
        || err.contains("did not answer")
        || err.contains("timed out")
        || err.contains("os error 10060")
    {
        return "browser did not answer".into();
    }
    if err.contains("refused") || err.contains("os error 10061") {
        return "connection refused".into();
    }
    err.chars().take(80).collect()
}

fn dial(host: &str, port: u16) -> Result<TcpStream, String> {
    use std::io::ErrorKind;
    use std::net::ToSocketAddrs;
    let addr = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|err| brief_net(&err.to_string()))?
        .next()
        .ok_or_else(|| "browser did not answer".to_string())?;
    match TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
        Ok(stream) => Ok(stream),
        Err(err) => Err(match err.kind() {
            ErrorKind::TimedOut => "browser did not answer".into(),
            ErrorKind::ConnectionRefused => "connection refused".into(),
            _ => brief_net(&err.to_string()),
        }),
    }
}

fn http_get(port: u16, path: &str) -> Result<String, String> {
    let mut stream = dial("127.0.0.1", port)?;
    stream
        .set_read_timeout(Some(Duration::from_millis(700)))
        .map_err(|err| err.to_string())?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(req.as_bytes())
        .map_err(|err| err.to_string())?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
        if buf.len() > 100_000 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    Ok(text.split("\r\n\r\n").nth(1).unwrap_or(&text).to_string())
}

fn read_n(stream: &mut TcpStream, len: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return Err("socket closed".into()),
            Ok(n) => filled += n,
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(buf)
}

fn mask4() -> [u8; 4] {
    let all = mask_bytes();
    [all[0], all[1], all[2], all[3]]
}

fn mask_bytes() -> [u8; 16] {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(1);
    let mut out = [0u8; 16];
    let mut value = n;
    for byte in &mut out {
        value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (value >> 33) as u8;
    }
    out
}

fn json_str(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn json_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = text.find(&needle)?;
    let after = text[start + needle.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let rest = after.strip_prefix('"')?;
    unescape(rest).map(|(value, _)| value)
}

fn json_string_value(text: &str) -> Option<String> {
    let at = text.find("\"type\":\"string\"")?;
    json_field(&text[at..], "value")
}

fn unescape(input: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut chars = input.chars();
    let mut used = 0usize;
    while let Some(ch) = chars.next() {
        used += ch.len_utf8();
        if ch == '"' {
            return Some((out, used));
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let next = chars.next()?;
        used += next.len_utf8();
        match next {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '\\' | '"' | '/' => out.push(next),
            'u' => {
                let mut hex = String::new();
                for _ in 0..4 {
                    let h = chars.next()?;
                    used += h.len_utf8();
                    hex.push(h);
                }
                let code = u32::from_str_radix(&hex, 16).ok()?;
                out.push(char::from_u32(code).unwrap_or('?'));
            }
            other => out.push(other),
        }
    }
    None
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let n = ((data[index] as u32) << 16) | ((data[index + 1] as u32) << 8) | data[index + 2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        index += 3;
    }
    if index < data.len() {
        let rem = data.len() - index;
        let mut n = (data[index] as u32) << 16;
        if rem == 2 {
            n |= (data[index + 1] as u32) << 8;
        }
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if rem == 2 {
            out.push(T[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push('=');
            out.push('=');
        }
    }
    out
}

fn b64_decode(data: &str) -> Option<Vec<u8>> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = data.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
    if bytes.len() < 4 {
        return None;
    }
    let mut out = Vec::new();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        let (a, b, c, d) = (bytes[index], bytes[index + 1], bytes[index + 2], bytes[index + 3]);
        let av = val(a)?;
        let bv = val(b)?;
        let cv = if c == b'=' { 0 } else { val(c)? };
        let dv = if d == b'=' { 0 } else { val(d)? };
        out.push((av << 2) | (bv >> 4));
        if c != b'=' {
            out.push((bv << 4) | (cv >> 2));
        }
        if d != b'=' {
            out.push((cv << 6) | dv);
        }
        index += 4;
    }
    Some(out)
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h = [
        0x6745_2301u32,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_the_websocket_sample() {
        let digest = sha1(b"abc");
        assert_eq!(
            digest,
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
            ]
        );
        let magic = b"dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        assert_eq!(b64(&sha1(magic)), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn a_socket_keeps_the_page_after_the_launcher_exits() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut engine = Engine::default();
        assert!(!engine.alive());
        engine.stream = Some(stream);
        assert!(engine.child.is_none());
        assert!(engine.alive());
        assert_eq!(
            profile_name(std::path::Path::new("granttap-edge-12")).as_deref(),
            Some("granttap-edge-12")
        );
        assert!(profile_name(std::path::Path::new("msedge")).is_none());
        assert!(profile_name(std::path::Path::new("granttap-edge-")).is_none());
        assert!(profile_name(std::path::Path::new("granttap-edge-12x")).is_none());
    }

    #[test]
    fn a_closed_port_does_not_wait_out_the_tcp_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_busy(port));
        drop(listener);
        assert!(!port_busy(port));
        let start = Instant::now();
        let err = dial("127.0.0.1", port).unwrap_err();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{err} {:?}",
            start.elapsed()
        );
        assert!(
            err == "connection refused" || err == "browser did not answer",
            "{err} {:?}",
            start.elapsed()
        );
        let wall = "A connection attempt failed because the connected party did not properly respond after a period of time, or established connection failed because connected host has failed to respond.";
        assert_eq!(brief_net(wall), "browser did not answer");
    }

    #[test]
    fn a_huge_image_box_does_not_cover_the_page() {
        let picture = "█".repeat(80);
        let picture = format!("{picture}\n").repeat(40);
        let nodes = vec![
            Node {
                kind: "img".into(),
                x: 0,
                y: 0,
                w: 2000,
                h: 2000,
                text: "logo.png".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 16,
                y: 16 * 14,
                w: 80,
                h: 16,
                text: "Search".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[Some(picture)], 80, 24);
        let block_rows = view.lines.iter().filter(|line| line.contains('█')).count();
        assert!(block_rows > 0 && block_rows <= 12, "{block_rows}");
        assert!(
            view.lines.iter().any(|line| line.contains("Search")),
            "{:?}",
            view.lines
        );
        let (_, _, wide, high) = cell_box(&nodes[0], 80, 24).unwrap();
        assert!(wide <= 64 && high <= 12, "{wide} {high}");
    }

    #[test]
    fn a_field_and_a_link_land_on_their_cells() {
        let nodes = vec![
            Node {
                kind: "text".into(),
                x: 16,
                y: 32,
                w: 80,
                h: 20,
                text: "Hello desk".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "field".into(),
                x: 16,
                y: 64,
                w: 200,
                h: 32,
                text: "Find".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "link".into(),
                x: 16,
                y: 112,
                w: 80,
                h: 18,
                text: "Example".into(),
                extra: "https://example.com/".into(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[], 80, 16);
        let joined = view.lines.join("\n");
        assert!(joined.contains("Hello desk"), "{joined}");
        assert!(joined.contains("[Find"), "{joined}");
        assert!(joined.contains("Example"), "{joined}");
        assert!(view.hits.iter().any(|hit| hit.kind == KIND_FIELD && hit.y == 4));
        assert!(view
            .hits
            .iter()
            .any(|hit| hit.kind == KIND_LINK && hit.h >= 2 && hit.href.contains("example.com")));
    }

    #[test]
    fn a_tall_button_covers_its_label_row() {
        let nodes = vec![Node {
            kind: "button".into(),
            x: 8,
            y: 8,
            w: 96,
            h: 48,
            text: "Search".into(),
            extra: String::new(),
            align: 'C',
            fg: String::new(),
            bg: "ffffff".into(),
            bd: "dadce0".into(),
        }];
        let view = compose(&nodes, &[], 40, 12);
        let hit = view
            .hits
            .iter()
            .find(|hit| hit.kind == KIND_BUTTON)
            .expect("button");
        assert!(hit.h >= 2, "{}", hit.h);
        let label_row = hit.y + hit.h / 2;
        let plain = strip_ink(&view.lines[label_row]);
        assert!(plain.contains("Search"), "{plain}");
        assert!(LAYOUT.contains("[onclick]"), "onclick hosts");
        assert!(LAYOUT.contains("role=tab"), "tab hosts");
        assert!(LAYOUT.contains("cursor!=='pointer'"), "pointer hosts");
        assert!(LAYOUT.contains("function clickHost"), "image hosts");
        assert!(LAYOUT.contains("function front"), "covered nodes stay hidden");
        assert!(LAYOUT.contains("function painted"), "pictures ignore aria-hidden");
        assert!(LAYOUT.contains("contentVisibility"), "hidden content stays out");
        assert!(LAYOUT.contains("inkGone"), "transparent text stays out");
        assert!(LAYOUT.contains("overflowY"), "a clip keeps the visible slice");
        assert!(LAYOUT.contains("frontImage"), "a transparent svg still counts");
        assert!(LAYOUT.contains("return ok>0"), "one hit keeps the picture");
        assert!(LAYOUT.contains("frontImage(bel,br)"), "background pictures stay");
        assert!(LAYOUT.contains("toUpperCase()"), "svg tagName is lowercase");
    }

    #[test]
    fn icon_buttons_keep_the_field_label() {
        let nodes = vec![
            Node {
                kind: "field".into(),
                x: 80,
                y: 160,
                w: 480,
                h: 44,
                text: "Найти".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: "ffffff".into(),
                bd: "1a73e8".into(),
            },
            Node {
                kind: "button".into(),
                x: 460,
                y: 168,
                w: 36,
                h: 28,
                text: "Голосовой поиск".into(),
                extra: String::new(),
                align: 'C',
                fg: String::new(),
                bg: "303134".into(),
                bd: String::new(),
            },
            Node {
                kind: "button".into(),
                x: 500,
                y: 168,
                w: 36,
                h: 28,
                text: "Поиск по картинке".into(),
                extra: String::new(),
                align: 'C',
                fg: String::new(),
                bg: "303134".into(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[], 120, 30);
        let plain = view.lines.iter().map(|line| strip_ink(line)).collect::<Vec<_>>().join("\n");
        assert!(plain.contains("Найти"), "{plain}");
        assert!(!plain.contains("Голос"), "{plain}");
        assert!(!plain.contains("картин"), "{plain}");
        assert!(plain.contains('•'), "{plain}");
    }

    #[test]
    fn a_focus_ring_stays_a_border() {
        let nodes = vec![
            Node {
                kind: "bar".into(),
                x: 76,
                y: 156,
                w: 488,
                h: 52,
                text: String::new(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: "1a73e8".into(),
                bd: "1a73e8".into(),
            },
            Node {
                kind: "field".into(),
                x: 80,
                y: 160,
                w: 480,
                h: 44,
                text: "Найти".into(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: "1a73e8".into(),
                bd: "8ab4f8".into(),
            },
        ];
        let view = compose(&nodes, &[], 120, 30);
        let row = view
            .lines
            .iter()
            .find(|line| strip_ink(line).contains("Найти"))
            .expect("field");
        assert!(row.contains("48;2;255;255;255"), "{row}");
        assert!(!row.contains("48;2;26;115;232"), "{row}");
        assert_eq!(field_paper(Some(0x303134), Some(0x5f6368)), 0x303134);
        assert_eq!(field_paper(Some(0x1a73e8), Some(0x8ab4f8)), 0xffffff);
        assert_eq!(field_paper(None, None), 0xffffff);
    }

    #[test]
    fn a_field_sliver_does_not_paint_beside_the_box() {
        let nodes = vec![
            Node {
                kind: "bar".into(),
                x: 40,
                y: 160,
                w: 72,
                h: 44,
                text: String::new(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: "ffffff".into(),
                bd: "dadce0".into(),
            },
            Node {
                kind: "field".into(),
                x: 120,
                y: 160,
                w: 400,
                h: 44,
                text: "Найти".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: "ffffff".into(),
                bd: "1a73e8".into(),
            },
        ];
        let view = compose(&nodes, &[], 80, 24);
        let row = view
            .lines
            .iter()
            .find(|line| strip_ink(line).contains("Найти"))
            .expect("field");
        let plain = strip_ink(row);
        let at = plain.find("Найти").expect("label");
        assert!(!plain[..at].contains('┌'), "{plain}");
        assert!(!plain[..at].contains('─'), "{plain}");
    }

    #[test]
    fn adjacent_scripts_do_not_share_cells() {
        let nodes = vec![
            Node {
                kind: "link".into(),
                x: 400,
                y: 320,
                w: 48,
                h: 18,
                text: "עברית".into(),
                extra: "https://he.example/".into(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "link".into(),
                x: 440,
                y: 320,
                w: 72,
                h: 18,
                text: "English".into(),
                extra: "https://example.com/".into(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[], 80, 28);
        let plain = view
            .lines
            .iter()
            .map(|line| strip_ink(line))
            .find(|line| line.contains("English"))
            .expect("english");
        let hebrew = plain.find('ע').expect("hebrew");
        let english = plain.find('E').expect("english");
        assert!(hebrew < english, "{plain}");
        assert!(
            !plain[hebrew..english].chars().any(|ch| ch.is_ascii_alphabetic()),
            "{plain}"
        );
        assert!(plain.contains("עברית"), "{plain}");
    }

    #[test]
    fn a_blank_tile_is_not_a_picture() {
        let white = image::RgbImage::from_pixel(32, 32, image::Rgb([255, 255, 255]));
        assert!(washed(&image::DynamicImage::ImageRgb8(white)));
        let mut mark = image::RgbImage::from_pixel(64, 32, image::Rgb([255, 255, 255]));
        for x in 8..56 {
            for y in 8..24 {
                mark.put_pixel(x, y, image::Rgb([0x31, 0x86, 0xFF]));
            }
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(mark)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        assert!(picture_of(buf.get_ref(), 8, 2).is_some());
    }

    #[test]
    #[ignore = "opens Edge and loads Google"]
    fn google_wordmark_is_painted() {
        let mut engine = Engine::default();
        engine.set_policy(false, true);
        engine.set_look(false, "ru-RU");
        let view = engine
            .show("https://www.google.com/?hl=ru", 120, 36)
            .expect("page");
        let pictured = view
            .lines
            .iter()
            .filter(|line| crate::draw::has_blocks(line))
            .count();
        assert!(pictured >= 2, "pictured {pictured} url {}", view.url);
        let plain = view
            .lines
            .iter()
            .map(|line| strip_ink(line))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!plain.contains("Голос"), "{plain}");
        assert!(!plain.contains("[["), "{plain}");
        engine.close();
    }

    #[test]
    fn a_long_label_stays_inside_its_box() {
        let nodes = vec![
            Node {
                kind: "link".into(),
                x: 80,
                y: 16,
                w: 40,
                h: 16,
                text: "Go".into(),
                extra: "https://example.com/go".into(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 0,
                y: 16,
                w: 32,
                h: 16,
                text: "ABCDEFGHIJKLMNOPQRSTUVWXYZ".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[], 40, 8);
        let plain = strip_ink(&view.lines[1]);
        assert!(plain.contains("ABCD"), "{plain}");
        assert!(!plain.contains("EFG"), "{plain}");
        assert!(plain.contains("Go"), "{plain}");
    }

    #[test]
    fn a_right_aligned_label_ends_the_box() {
        let nodes = vec![Node {
            kind: "text".into(),
            x: 0,
            y: 0,
            w: 80,
            h: 16,
            text: "XY".into(),
            extra: String::new(),
            align: 'R',
            fg: String::new(),
            bg: String::new(),
            bd: String::new(),
        }];
        let view = compose(&nodes, &[], 20, 4);
        let plain = strip_ink(&view.lines[0]);
        assert!(plain.trim_end().ends_with("XY"), "{plain}");
        assert!(!plain.starts_with("XY"), "{plain}");
    }

    #[test]
    fn a_page_washes_the_ground_and_a_field_keeps_its_frame() {
        let nodes = vec![
            Node {
                kind: "wash".into(),
                x: 0,
                y: 0,
                w: 640,
                h: 320,
                text: String::new(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: "ffffff".into(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 16,
                y: 16,
                w: 80,
                h: 16,
                text: "Hello".into(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "field".into(),
                x: 16,
                y: 64,
                w: 240,
                h: 48,
                text: "Search".into(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: "ffffff".into(),
                bd: "dadce0".into(),
            },
            Node {
                kind: "text".into(),
                x: 16,
                y: 200,
                w: 80,
                h: 16,
                text: "Footer".into(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: String::new(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[], 80, 24);
        let joined = view.lines.join("\n");
        assert!(joined.contains("48;2;255;255;255"), "{joined}");
        assert!(joined.contains("38;2;32;33;36"), "{joined}");
        assert!(joined.contains("Hello"), "{joined}");
        assert!(joined.contains("Search"), "{joined}");
        assert!(joined.contains('┌') || joined.contains('─'), "{joined}");
        let hello = view
            .lines
            .iter()
            .position(|line| strip_ink(line).contains("Hello"))
            .unwrap();
        let footer = view
            .lines
            .iter()
            .position(|line| strip_ink(line).contains("Footer"))
            .unwrap();
        assert!(hello < footer, "{hello} {footer}");
        assert!(view.hits.iter().any(|hit| hit.kind == KIND_FIELD && hit.w >= 20));
    }

    fn strip_ink(line: &str) -> String {
        let mut out = String::new();
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for next in chars.by_ref() {
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    #[test]
    fn hebrew_stays_beside_latin_and_scrolled_lines_do_not_mix() {
        let nodes = vec![
            Node {
                kind: "text".into(),
                x: 0,
                y: 0,
                w: 240,
                h: 16,
                text: "Consent שלום Gmail".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 0,
                y: 112,
                w: 64,
                h: 14,
                text: "AAAA".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 0,
                y: 120,
                w: 64,
                h: 14,
                text: "BBBB".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[], 40, 16);
        let joined = view.lines.join("\n");
        assert!(joined.contains("שלום"), "{joined}");
        assert!(!joined.contains("םולש"), "{joined}");
        let at = joined.find("שלום").expect("hebrew");
        assert_eq!(
            joined[..at].chars().next_back(),
            Some('\u{2067}'),
            "{joined}"
        );
        assert!(joined.contains('\u{2066}'), "{joined}");
        assert!(joined.contains("Gmail"), "{joined}");
        let mixed = view.lines.iter().any(|line| {
            let plain = strip_ink(line);
            plain.contains('A') && plain.contains('B')
        });
        assert!(!mixed, "{joined}");
        assert!(view.lines.iter().any(|line| strip_ink(line).contains("AAAA")));
        assert!(view.lines.iter().any(|line| strip_ink(line).contains("BBBB")));
    }

    #[test]
    fn scroll_snaps_and_a_google_page_takes_the_chosen_language() {
        assert_eq!(wheel_delta(1), 64);
        assert_eq!(wheel_delta(-1), -64);
        assert_eq!(wheel_delta(1) % 16, 0);
        let js = wheel_js(64);
        assert!(js.contains("elementFromPoint"), "{js}");
        assert!(js.contains("snap"), "{js}");
        assert!(js.contains("innerHeight*0.55"), "{js}");
        assert!(!js.contains("querySelectorAll"), "{js}");
        let icon = Node {
            kind: "img".into(),
            x: 400,
            y: 120,
            w: 28,
            h: 28,
            text: "SVG".into(),
            extra: String::new(),
            align: 'L',
            fg: String::new(),
            bg: String::new(),
            bd: String::new(),
        };
        let moved = Node {
            kind: "img".into(),
            x: 400,
            y: 40,
            w: 28,
            h: 28,
            text: "SVG".into(),
            extra: String::new(),
            align: 'L',
            fg: String::new(),
            bg: String::new(),
            bd: String::new(),
        };
        assert_eq!(shot_key(&icon), shot_key(&moved));
        assert_ne!(shot_key(&icon), shot_key(&Node { x: 80, ..moved }));
        assert_eq!(
            prefer_google_lang("https://www.google.com/?zx=1794333350506", "en-US"),
            "https://www.google.com/?zx=1794333350506&hl=en"
        );
        assert_eq!(
            prefer_google_lang("https://www.google.co.il/search?q=a&hl=iw", "ru-RU"),
            "https://www.google.co.il/search?q=a&hl=ru"
        );
        assert_eq!(
            prefer_google_lang("https://example.com/docs", "ru-RU"),
            "https://example.com/docs"
        );
        assert_eq!(accept_language("en-US"), "en-US,en;q=0.9");
        assert_eq!(accept_language("ru-RU"), "ru-RU,ru;q=0.9,en;q=0.5");
        assert_eq!(accept_language("he-IL"), "he-IL,he;q=0.9,en;q=0.5");
        let chrome = plain_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/131.0.0.0 Safari/537.36",
        );
        assert!(!chrome.to_ascii_lowercase().contains("headless"), "{chrome}");
        assert!(chrome.contains("Chrome/131"), "{chrome}");
        let edge = plain_agent("Mozilla/5.0 HeadlessEdg/131.0.0.0");
        assert!(edge.contains("Edg/131") && !edge.contains("Headless"), "{edge}");
        assert!(plain_agent("").contains("Edg/"));
        assert!(script_blocked(
            "exception: script execution is disabled"
        ));
        assert!(!script_blocked("Runtime.evaluate timed out"));
        let sentence = "Мы зарегистрировали подозрительный трафик с вашей сети";
        let fit = fit_text(sentence, 28);
        assert!(fit.chars().count() <= 28, "{fit}");
        assert!(sentence.starts_with(&fit), "{fit}");
        let rest = sentence.strip_prefix(&fit).unwrap();
        assert!(rest.is_empty() || rest.starts_with(' '), "{fit}");
        let url = "https://www.google.com/sorry/index?continue=https%3A%2F%2Fwww.google.com";
        let short = fit_text(url, 40);
        assert!(short.chars().count() <= 40, "{short}");
        assert!(url.starts_with(&short), "{short}");
        let tail = url.strip_prefix(&short).unwrap();
        let last = short.chars().last().unwrap();
        let next = tail.chars().next();
        assert!(
            next.is_none()
                || last.is_whitespace()
                || next.is_some_and(|ch| ch.is_whitespace())
                || matches!(last, '/' | '?' | '&' | '=' | '-' | ',' | '.'),
            "{short}"
        );
        assert_eq!(fit_text(&"A".repeat(40), 10).chars().count(), 10);
    }

    #[test]
    fn edge_lays_out_a_page_with_a_field() {
        if browser_path().is_none() {
            return;
        }
        let mut engine = Engine::default();
        let view = engine
            .show(
                "data:text/html,<h1>Hello%20desk</h1><input%20aria-label=Find><a%20href=https://example.com/>Example</a>",
                80,
                24,
            )
            .expect("edge");
        let joined = view.lines.join("\n");
        assert!(
            joined.contains("Hello desk") || joined.contains("Hello"),
            "{joined}"
        );
        assert!(
            view.hits.iter().any(|hit| hit.kind == KIND_FIELD),
            "{:?}",
            view.hits.iter().map(|hit| hit.label.clone()).collect::<Vec<_>>()
        );
        assert!(
            view.hits
                .iter()
                .any(|hit| hit.kind == KIND_LINK && hit.href.contains("example.com")),
            "{:?}",
            view.hits
        );
    }

    fn data_html(body: &str) -> String {
        let mut url = String::from("data:text/html;charset=utf-8,");
        for byte in body.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => url.push(byte as char),
                _ => url.push_str(&format!("%{byte:02X}")),
            }
        }
        url
    }

    #[test]
    fn a_dialog_covers_the_page_and_the_field_stays_a_box() {
        if browser_path().is_none() {
            return;
        }
        let page = data_html(
            r#"<!doctype html><html><body style="margin:0;background:#ffffff;color:#202124">
<h1 style="position:absolute;left:80px;top:120px;margin:0">Behind</h1>
<div style="position:fixed;inset:0;background:#000;opacity:0.45"></div>
<div role="dialog" style="position:fixed;left:80px;top:64px;width:360px;height:140px;background:#ffffff">
<p style="margin:12px">Consent שלום Gmail</p><button type="button">Accept</button>
</div>
<input aria-label="Search" style="position:fixed;left:80px;bottom:48px;width:240px;height:36px">
<div style="position:fixed;left:0;bottom:0;height:28px;width:100%;background:#eeeeee">Footer</div>
</body></html>"#,
        );
        let mut engine = Engine::default();
        let view = engine.show(&page, 100, 30).expect("edge");
        let plain: Vec<String> = view.lines.iter().skip(1).map(|line| strip_ink(line)).collect();
        let joined = plain.join("\n");
        assert!(joined.contains("Consent"), "{joined}");
        assert!(joined.contains("שלום"), "{joined}");
        assert!(joined.contains("Gmail"), "{joined}");
        assert!(!joined.contains("םולש"), "{joined}");
        assert!(!joined.contains("liamG"), "{joined}");
        assert!(!joined.contains("Behind"), "{joined}");
        assert!(joined.contains("Accept"), "{joined}");
        assert!(joined.contains("Footer"), "{joined}");
        assert!(
            view.hits.iter().any(|hit| hit.kind == KIND_FIELD),
            "{:?}",
            view.hits.iter().map(|hit| &hit.label).collect::<Vec<_>>()
        );
        let ink = view.lines.join("\n");
        assert!(
            ink.contains("48;2;255;255;255") || ink.contains("48;2;238;238;238"),
            "{ink}"
        );
        let consent = plain.iter().position(|line| line.contains("Consent")).unwrap();
        let footer = plain.iter().position(|line| line.contains("Footer")).unwrap();
        assert!(consent < footer, "{consent} {footer}\n{joined}");
    }

    #[test]
    fn a_scrolled_off_block_does_not_cover_the_page() {
        let nodes = vec![
            Node {
                kind: "bar".into(),
                x: 0,
                y: -160,
                w: 320,
                h: 200,
                text: String::new(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: "ff0000".into(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 8,
                y: -40,
                w: 80,
                h: 80,
                text: "GoneGone".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 8,
                y: 32,
                w: 80,
                h: 16,
                text: "Visible".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 8,
                y: 48,
                w: 48,
                h: 16,
                text: "AAA".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 8,
                y: 50,
                w: 48,
                h: 16,
                text: "BBB".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
            Node {
                kind: "text".into(),
                x: 8,
                y: 52,
                w: 48,
                h: 16,
                text: "CCC".into(),
                extra: String::new(),
                align: 'L',
                fg: String::new(),
                bg: String::new(),
                bd: String::new(),
            },
        ];
        let view = compose(&nodes, &[], 40, 12);
        let joined = view.lines.join("\n");
        assert!(joined.contains("Visible"), "{joined}");
        assert!(!joined.contains("GoneGone"), "{joined}");
        let red = view
            .lines
            .iter()
            .filter(|line| line.contains("48;2;255;0;0"))
            .count();
        assert!(red < 5, "scrolled bar covered {red} rows\n{joined}");
        assert!(view.lines.iter().any(|line| strip_ink(line).contains("AAA")));
        assert!(view.lines.iter().any(|line| strip_ink(line).contains("BBB")));
        let mixed = view.lines.iter().any(|line| {
            let plain = strip_ink(line);
            plain.contains('C') && (plain.contains('A') || plain.contains('B'))
        });
        assert!(!mixed, "{joined}");
        let focus = focus_js(12, 24);
        assert!(focus.contains("elementsFromPoint"), "{focus}");
        assert!(!focus.contains("document.querySelector"), "{focus}");
        let write = field_js(12, 24, "desk", false);
        assert!(write.contains("selectionStart"), "{write}");
        assert!(write.contains("elementsFromPoint"), "{write}");
        assert!(!write.contains("document.querySelector"), "{write}");
        assert!(!write.contains("Input.insertText"), "{write}");
    }

    #[test]
    fn a_light_address_uses_page_paper_and_a_ring_stays_clear() {
        let head = address_line("https://www.google.com/", Some(0x202124), Some(0xffffff));
        assert!(head.contains("48;2;255;255;255"), "{head}");
        assert!(head.contains("38;2;32;33;36"), "{head}");
        assert!(!head.contains("38;5;131"), "{head}");
        let nodes = vec![
            Node {
                kind: "wash".into(),
                x: 0,
                y: 0,
                w: 640,
                h: 400,
                text: String::new(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: "ffffff".into(),
                bd: String::new(),
            },
            Node {
                kind: "bar".into(),
                x: 40,
                y: 80,
                w: 280,
                h: 48,
                text: String::new(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: "1a1a1a".into(),
                bd: "1a1a1a".into(),
            },
            Node {
                kind: "field".into(),
                x: 48,
                y: 84,
                w: 240,
                h: 36,
                text: "Find".into(),
                extra: String::new(),
                align: 'L',
                fg: "202124".into(),
                bg: "ffffff".into(),
                bd: "dadce0".into(),
            },
        ];
        let view = compose(&nodes, &[], 80, 24);
        let ink = view.lines.join("\n");
        assert!(
            !ink.contains("48;2;26;26;26"),
            "the focus ring is not a dark slab\n{ink}"
        );
        assert!(ink.contains("Find") || ink.contains("[Find"), "{ink}");
    }

    #[test]
    fn a_field_takes_typed_text() {
        if browser_path().is_none() {
            return;
        }
        let mut engine = Engine::default();
        let view = engine
            .show(
                "data:text/html,<input aria-label=Find style=\"position:fixed;left:40px;top:40px;width:240px;height:32px\">",
                80,
                24,
            )
            .expect("edge");
        let hit = view
            .hits
            .iter()
            .find(|hit| hit.kind == KIND_FIELD)
            .expect("field");
        engine.focus_at(hit.px, hit.py).expect("focus");
        engine.insert("desk").expect("insert");
        let again = engine.refresh(80, 24, false).expect("refresh");
        let joined = again.lines.join("\n");
        assert!(joined.contains("desk"), "{joined}");
    }
}
