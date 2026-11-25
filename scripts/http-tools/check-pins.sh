# Shared prerequisites for setup and offline verification.
require_version() {
    local tool=$1 expected=$2 actual
    shift 2
    if ! actual=$("$tool" "$@"); then
        printf 'Required tool: %s (%s)\n' "$tool" "$expected" >&2
        exit 1
    fi
    case "$actual" in
        "$expected"|"$expected "*) ;;
        *)
            printf '%s: expected %s, found %s\n' "$tool" "$expected" "$actual" >&2
            exit 1
            ;;
    esac
}

check_http_runtime() {
    require_version rustc 'rustc 1.96.1' --version
    require_version node v22.22.2 --version
    require_version npm 10.9.7 --version
    require_version go 'go version go1.26.3' version
    if ! command -v cargo >/dev/null; then
        printf 'Required tool: cargo (Rust 1.96.1 toolchain)\n' >&2
        exit 1
    fi
}

check_http_tools() (
    cd "$1"
    require_version ./node_modules/.bin/tsc 'Version 5.9.3' --version
    require_version ./node_modules/.bin/playwright 'Version 1.58.2' --version
    require_version node 8.20.0 -p 'require("ajv/package.json").version'
    require_version node 145.0.7632.6 -e '
        (async () => {
            const browser = await require("playwright").chromium.launch({headless: true});
            try {
                console.log(browser.version());
            } finally {
                await browser.close();
            }
        })().catch(error => {
            console.error("Required browser: Playwright Chromium 145.0.7632.6", error);
            process.exitCode = 1;
        });
    '
)

check_hybrid_tools() (
    cd "$1"
    require_version ./node_modules/.bin/vite 'vite/8.3.8' --version
    require_version ./node_modules/.bin/playwright 'Version 1.58.2' --version
    require_version node 19.2.7 -p 'require("react/package.json").version'
    require_version node 19.2.7 -p 'require("react-dom/package.json").version'
)

http_schema=openapi-3.1-2022-10-07.json
http_schema_sha=da01ba28852cac0de53893797cb8d1942bc3b05084f526dcc216717dec314ed0
check_http_schema() (
    cd "$1"
    printf '%s  %s\n' "$http_schema_sha" "$http_schema" | shasum -a 256 --check
)
