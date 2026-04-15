//! Unix 一行 → PowerShell 启发式（`split-window` / `send-keys`）。

use regex::Regex;

pub(crate) fn convert_unix_to_powershell(input: &str) -> String {
    let result = input.to_string();

    // 1. 先处理环境变量: $VAR -> $env:VAR
    // 需要小心处理，避免转换已有的 $env:VAR
    let result = convert_env_variables(&result);

    // 2. 处理常见的 Unix 命令和语法
    let result = convert_shell_syntax(&result);

    result
}

/// 转换环境变量引用
fn convert_env_variables(input: &str) -> String {
    let result = input.to_string();

    // 匹配 ${VAR} 或 $VAR 模式，但不匹配 $env:VAR
    // 使用正则表达式风格的替换
    let pattern_regex = Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();

    // 替换 $VAR 为 $env:VAR，但跳过已经是 $env:VAR 的情况
    let mut output = String::new();
    let mut last_end = 0;

    for mat in pattern_regex.find_iter(&result) {
        // 检查前面是否有 $env: 前缀
        let prefix_start = mat.start().saturating_sub(6);
        if prefix_start < last_end || !result[prefix_start..mat.start()].ends_with("$env:") {
            output.push_str(&result[last_end..mat.start()]);
            output.push_str("$env:");
            output.push_str(&mat.as_str()[1..]); // 去掉开头的 $
            last_end = mat.end();
        } else {
            output.push_str(&result[last_end..mat.end()]);
            last_end = mat.end();
        }
    }
    output.push_str(&result[last_end..]);

    // 处理 ${VAR} 形式
    let pattern_braces = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
    let mut output2 = String::new();
    let mut last_end = 0;

    for mat in pattern_braces.find_iter(&output) {
        output2.push_str(&output[last_end..mat.start()]);
        output2.push_str("$env:");
        output2.push_str(&mat.as_str()[2..mat.as_str().len() - 1]); // 去掉 ${ 和 }
        last_end = mat.end();
    }
    output2.push_str(&output[last_end..]);

    output2
}

/// 转换 shell 语法
/// 处理独立的 env 命令: env VAR=value command -> $env:VAR=value; command
/// 处理 env 命令: env VAR1=value1 VAR2=value2 -> $env:VAR1=value1; $env:VAR2=value2
/// 同时处理末尾的命令，例如: env VAR=value cmd -> $env:VAR=value; cmd
fn convert_env_command_standalone(input: &str) -> String {
    // 先把开头的 "env " 替换掉
    let input = input.strip_prefix("env ").unwrap_or(input);

    // 匹配所有 KEY=VALUE 对
    let var_regex = Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)=([^\s]+)").unwrap();

    let mut has_env = false;
    let mut output = String::new();
    let mut last_end = 0;

    for cap in var_regex.captures_iter(input) {
        let mat = cap.get(0).unwrap();
        output.push_str(&input[last_end..mat.start()]);

        let var_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let value = cap.get(2).map(|m| m.as_str()).unwrap_or("");

        // 去掉值中的引号
        let value = value.trim_matches('"').trim_matches('\'');

        output.push_str(&format!("$env:{}=\"{}\"", var_name, value));
        has_env = true;
        last_end = mat.end();

        // 添加分号分隔多个环境变量或命令
        if last_end < input.len() {
            let next_char = input.chars().nth(last_end).unwrap_or(' ');
            if next_char != ';' && next_char != '&' {
                output.push(';');
            }
        }
    }

    if has_env {
        output.push_str(&input[last_end..]);
        output
    } else {
        input.to_string()
    }
}

fn convert_shell_syntax(input: &str) -> String {
    let mut result = input.to_string();

    // 避免重复转换：检查是否已经包含 PowerShell 特征
    let already_ps = result.contains("$env:")
        || result.contains("Get-ChildItem")
        || result.contains("Set-Location")
        || result.contains("Write-Output");
    if already_ps {
        return result;
    }

    // ====== 基础转换 ======

    // 0.1 转换 && 到 ; (顺序执行) - PowerShell 不支持 &&
    result = result.replace("&&", ";");

    // 0.2 转换 || 到 ; - PowerShell 也不支持 || (用 ; 替代顺序执行)
    result = result.replace("||", ";");

    // 0.3 处理 env 命令: env VAR=value command -> $env:VAR=value; command
    result = convert_env_command_standalone(&result);

    // 0.4 处理 cd 后面跟 && 的情况: cd path && -> cd path;
    if result.contains("cd ") && result.contains(" && ") {
        result = result.replace(" && ", " ; ");
    }

    // 0.5 处理 ~ 转换
    result = result.replace("$HOME", "$HOME"); // 保留，让 PowerShell 处理

    // 2.1 转换 cd 命令
    if result.starts_with("cd ") {
        result = convert_cd_command(&result);
    }

    // 2.2 转换 export VAR=value
    if result.starts_with("export ") {
        result = convert_export_command(&result);
    }

    // 2.3 处理 ls 命令及参数
    result = convert_ls_command(&result);

    // 2.4 处理 cat 命令
    result = convert_cat_command(&result);

    // 2.5 处理 rm 命令
    result = convert_rm_command(&result);

    // 2.6 处理 cp 命令
    result = convert_cp_command(&result);

    // 2.7 处理 mv 命令
    result = convert_mv_command(&result);

    // 2.8 处理 mkdir 命令
    result = convert_mkdir_command(&result);

    // 2.9 处理 pwd 命令
    result = convert_pwd_command(&result);

    // 2.10 处理 grep 命令
    result = convert_grep_command(&result);

    // 2.11 处理 echo 命令
    result = convert_echo_command(&result);

    // 2.12 处理 which 命令
    result = convert_which_command(&result);

    // 2.13 处理 chmod/chown (警告但保留)
    result = convert_permission_commands(&result);

    // 2.14 处理 source 命令
    result = convert_source_command(&result);

    // 2.15 处理管道和重定向
    result = convert_pipes_and_redirects(&result);

    result
}

fn convert_cd_command(input: &str) -> String {
    let result = input
        .replace("cd ~", "cd ~")
        .replace("cd $HOME", "cd ~")
        .replace("cd /", "cd C:/");

    if result.contains("~") && !result.contains("$HOME") {
        result.replace("~", "~")
    } else {
        result
    }
}

fn convert_export_command(input: &str) -> String {
    let input = input.strip_prefix("export ").unwrap_or(input);

    if let Some(eq_pos) = input.find('=') {
        let var_name = &input[..eq_pos];
        let value = &input[eq_pos + 1..];

        let value = value.trim_matches('"').trim_matches('\'');

        format!("$env:{}=\"{}\"", var_name, value)
    } else {
        input.to_string()
    }
}

fn convert_ls_command(input: &str) -> String {
    let input_lower = input.to_lowercase();

    if input_lower.starts_with("ls ") || input_lower == "ls" {
        let result = input
            .replace("ls -la", "Get-ChildItem -Force")
            .replace("ls -l", "Get-ChildItem -Force")
            .replace("ls -a", "Get-ChildItem -Force")
            .replace("ls", "Get-ChildItem");

        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.len() > 1 && !parts[1].starts_with('-') {
            result.replace("Get-ChildItem", &format!("Get-ChildItem -Path '{}'", parts[1]))
        } else {
            result
        }
    } else {
        input.to_string()
    }
}

fn convert_cat_command(input: &str) -> String {
    if input.starts_with("cat ") {
        let path = input.strip_prefix("cat ").unwrap_or("");
        format!("Get-Content -Path '{}'", path)
    } else {
        input.to_string()
    }
}

fn convert_rm_command(input: &str) -> String {
    if input.starts_with("rm ") {
        let result = input
            .replace("rm -rf ", "Remove-Item -Recurse -Force -Path '")
            .replace("rm -r ", "Remove-Item -Recurse -Path '")
            .replace("rm -f ", "Remove-Item -Force -Path '")
            .replace("rm ", "Remove-Item -Path '");

        if result.starts_with("Remove-Item") && !result.ends_with('\'') {
            result + "'"
        } else {
            result
        }
    } else {
        input.to_string()
    }
}

fn convert_cp_command(input: &str) -> String {
    if input.starts_with("cp ") {
        let result = input
            .replace("cp -r ", "Copy-Item -Recurse -Path '")
            .replace("cp -R ", "Copy-Item -Recurse -Path '")
            .replace("cp ", "Copy-Item -Path '");

        if result.starts_with("Copy-Item") && !result.ends_with('\'') {
            result
        } else {
            result
        }
    } else {
        input.to_string()
    }
}

fn convert_mv_command(input: &str) -> String {
    if input.starts_with("mv ") {
        let result = input.replace("mv ", "Move-Item -Path '");

        if result.starts_with("Move-Item") && !result.ends_with('\'') {
            result
        } else {
            result
        }
    } else {
        input.to_string()
    }
}

fn convert_mkdir_command(input: &str) -> String {
    if input.starts_with("mkdir ") {
        let path = input.replace("mkdir -p ", "").replace("mkdir ", "");

        format!("New-Item -ItemType Directory -Path '{}' -Force", path)
    } else {
        input.to_string()
    }
}

fn convert_pwd_command(input: &str) -> String {
    if input.trim() == "pwd" {
        "Get-Location".to_string()
    } else {
        input.to_string()
    }
}

fn convert_grep_command(input: &str) -> String {
    if input.contains("grep ") || input.contains(" grep ") {
        let result = input
            .replace("grep -i ", "Select-String -CaseSensitive:$false '")
            .replace("grep -v ", "Select-String -NotMatch '")
            .replace("grep ", "Select-String '");

        if result.starts_with("Select-String") && !result.ends_with('\'') {
            result + "'"
        } else {
            result
        }
    } else {
        input.to_string()
    }
}

fn convert_echo_command(input: &str) -> String {
    if input.starts_with("echo ") {
        let text = input.strip_prefix("echo ").unwrap_or("");
        let text = text
            .replace("\\n", "`n")
            .replace("\\t", "`t")
            .replace("\\\"", "`\"");

        format!("Write-Output \"{}\"", text)
    } else {
        input.to_string()
    }
}

fn convert_which_command(input: &str) -> String {
    if input.starts_with("which ") {
        let cmd = input.strip_prefix("which ").unwrap_or("");
        format!("Get-Command {} | Select-Object -ExpandProperty Source", cmd)
    } else {
        input.to_string()
    }
}

fn convert_permission_commands(input: &str) -> String {
    if input.starts_with("chmod ") || input.starts_with("chown ") {
        format!("# Unix permission command not supported on Windows: {}", input)
    } else {
        input.to_string()
    }
}

fn convert_source_command(input: &str) -> String {
    if input.starts_with("source ") {
        let file = input.strip_prefix("source ").unwrap_or("");
        format!(". '{}'", file)
    } else {
        input.to_string()
    }
}

fn convert_pipes_and_redirects(input: &str) -> String {
    let mut result = input.to_string();

    if result.contains(" | xargs") {
        result = result.replace(" | xargs", " | ForEach-Object");
    }

    if result.contains(" | head ") {
        let parts: Vec<&str> = result.split(" | head ").collect();
        if parts.len() == 2 {
            if let Ok(n) = parts[1].parse::<usize>() {
                result = format!("{} | Select-Object -First {}", parts[0], n);
            }
        }
    }

    if result.contains(" | tail ") {
        let parts: Vec<&str> = result.split(" | tail ").collect();
        if parts.len() == 2 {
            if let Ok(n) = parts[1].parse::<usize>() {
                result = format!("{} | Select-Object -Last {}", parts[0], n);
            }
        }
    }

    if result.contains(" | wc -l") {
        result = result
            .replace(" | wc -l", " | Measure-Object -Line")
            .replace("$", "( ).Lines");
    }

    result
}
