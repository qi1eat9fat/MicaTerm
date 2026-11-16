//! `micaterm` 命令单独成一个控制台程序（外部 CLI）。Windows 上应用本身是 GUI 程序，
//! shell 既不等它也看不到它的输出，所以命令行由这个程序承担。

fn main() {
    std::process::exit(micaterm::cli::main(std::env::args_os().skip(1).collect()));
}
