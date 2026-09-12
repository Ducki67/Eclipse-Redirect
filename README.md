# Eclipse Redirect

A low-profile 26.20 Redirect written in Rust.

It hooks `FCurlHttpRequest::ProcessRequest` in the client and in the EOS SDK, rewrites the host of
matching requests to your backend, and leaves the path and query untouched. No dependencies beyond
`windows-sys`, ~100 KB output.

## Build from source

**Requirements**

- [Rust](https://rustup.rs) (stable). On Windows rustup defaults to `x86_64-pc-windows-msvc`, which
  is the target you want.
- MSVC linker — install **Visual Studio Build Tools** with the *Desktop development with C++*
  workload.

**Build**

```powershell
.\build.ps1
```

That produces:

```
out\release\Eclipse Redirect.dll
```

## Configuration

Everything lives in [`src/opts.rs`](src/opts.rs). Change it, rebuild, done.

| Option | Default | What it does |
| --- | --- | --- |
| `BACKEND` | `http://127.0.0.1:3551` | Your backend URL. |
| `URL_SET` | `All` | What gets redirected — see below. |
| `CONSOLE` | `true` | Debug console window. Turn off for release/production builds. |
| `LOG_REQUESTS` | `true` | Log every URL and rewrite to the console and `%TEMP%\eclipse_redirect.log`. |
| `B_HAS_PUSH_WIDGET` | `false` | Fixes the game server closing seconds after it starts listening. **Breaks closing the client** — never ship it in a launcher build. |

`URL_SET` modes:

| Mode | Behaviour |
| --- | --- |
| `Default` | Epic's domains. The normal private server setup. |
| `Hybrid` | Only profile, version and content paths; everything else goes to Epic. |
| `Dev` | Only profile and content paths; everything else goes to Epic. |
| `All` | Every request. |

### Advanced

Leave these alone unless you know why you're changing them.

| Option | Default | What it does |
| --- | --- | --- |
| `B_USE_ARG_PARAMS` | `false` | Read the backend from the command line (`-backend=http://IP:PORT`). While on, `CONSOLE` is ignored and the console only opens with `-bConsole=true`. |
| `B_MANUAL_MAPPING` | `false` | Run init inline instead of on a new thread, for EAC with a manual mapper. |
| `PROCESS_REQUEST_VTABLE_RVA` | `0x0A793690` | `ProcessRequest` vtable slot, which skips the startup scan of `.text`. This is the 26.20 value; the function prologue is verified before it's trusted, so any other build falls back to scanning on its own. Set to `0` to always scan. |
| `SET_URL_INDEX` | `0` | `SetURL` vtable index. `0` detects it at runtime, which finds `10` on 26.20. |

## Usage

Inject the DLL into `FortniteClient-Win64-Shipping.exe` with any normal LoadLibrary injector
(Reboot Launcher works tho). With `CONSOLE` on you should see the debug logs and enjoy. :))

## Credits

Structure layout and url_set modes are from: [Starfall](https://github.com/plooshi/Starfall) by Ploosh.

## License

[MIT](LICENSE)
