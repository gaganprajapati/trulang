//! C Backend Code Generator for TruLang.
//!
//! Translates TruLang compiled bytecode chunks into ANSI C99 source code
//! that compiles with clang, gcc, or MSVC cl.exe to produce standalone native binaries.

use std::fmt::Write;
use trulang_compiler::bytecode::{Chunk, Opcode, Value};
use crate::runtime::TRULANG_RUNTIME_C;

pub struct CBackend;

impl CBackend {
    /// Compiles a TruLang Chunk into a standalone native C source file.
    pub fn emit(chunk: &Chunk) -> String {
        let mut out = String::with_capacity(16 * 1024);

        // 1. Embed TruLang Runtime Header & Implementations
        out.push_str(TRULANG_RUNTIME_C);
        out.push('\n');

        // 2. Determine capacity needs for locals, fields, and stack
        let mut max_local: usize = 0;
        let mut max_field: usize = 0;
        let mut _has_alloc_region_a = false;
        let mut init_budget: usize = 16 * 1024 * 1024;
        let mut init_max_budget: usize = 0;

        for op in &chunk.code {
            match op {
                Opcode::GetLocal(idx) | Opcode::SetLocal(idx) => {
                    if *idx > max_local {
                        max_local = *idx;
                    }
                }
                Opcode::GetField(idx) | Opcode::SetField(idx) => {
                    if *idx > max_field {
                        max_field = *idx;
                    }
                }
                Opcode::AllocRegionA(budget, max_b) => {
                    _has_alloc_region_a = true;
                    init_budget = *budget;
                    init_max_budget = max_b.unwrap_or(0);
                }
                _ => {}
            }
        }

        let locals_size = (max_local + 16).max(32);
        let fields_size = (max_field + 16).max(32);
        let stack_size = 4096;

        // 3. Emit Main Entrypoint Function
        out.push_str("/* ========================================================================= */\n");
        out.push_str("/*                         Generated TruLang Entrypoint                        */\n");
        out.push_str("/* ========================================================================= */\n\n");
        out.push_str("int main(int argc, char** argv) {\n");
        out.push_str("    (void)argc; (void)argv;\n\n");

        // Initialize Region A arena
        let _ = writeln!(out, "    trulang_region_a_init({}, {});", init_budget, init_max_budget);
        let _ = writeln!(out, "    TruLangVal locals[{}] = {{0}};", locals_size);
        let _ = writeln!(out, "    TruLangVal fields[{}] = {{0}};", fields_size);
        let _ = writeln!(out, "    TruLangVal stack[{}];", stack_size);
        out.push_str("    int sp = 0;\n");
        out.push_str("    TruLangVal result_val = trulang_val_null();\n\n");

        // Constants initialization
        let consts_len = chunk.constants.len();
        if consts_len > 0 {
            let _ = writeln!(out, "    TruLangVal consts[{}];", consts_len);
            for (i, c) in chunk.constants.iter().enumerate() {
                match c {
                    Value::Int(n) => {
                        let _ = writeln!(out, "    consts[{}] = trulang_val_int({}LL);", i, n);
                    }
                    Value::Float(f) => {
                        let _ = writeln!(out, "    consts[{}] = trulang_val_float({});", i, f);
                    }
                    Value::Bool(b) => {
                        let _ = writeln!(out, "    consts[{}] = trulang_val_bool({});", i, if *b { "true" } else { "false" });
                    }
                    Value::String(s) => {
                        let escaped = escape_c_string(s);
                        let _ = writeln!(out, "    consts[{}] = trulang_val_string(\"{}\", false);", i, escaped);
                    }
                    Value::Null => {
                        let _ = writeln!(out, "    consts[{}] = trulang_val_null();", i);
                    }
                    _ => {
                        let _ = writeln!(out, "    consts[{}] = trulang_val_null();", i);
                    }
                }
            }
            out.push('\n');
        }

        // 4. Translate Bytecode Opcodes to native C
        for (ip, op) in chunk.code.iter().enumerate() {
            let _ = writeln!(out, "label_{:04}:", ip);
            match op {
                Opcode::LoadConst(idx) => {
                    let _ = writeln!(out, "    stack[sp++] = consts[{}];", idx);
                }
                Opcode::LoadInt(n) => {
                    let _ = writeln!(out, "    stack[sp++] = trulang_val_int({}LL);", n);
                }
                Opcode::LoadFloat(f) => {
                    let _ = writeln!(out, "    stack[sp++] = trulang_val_float({});", f);
                }
                Opcode::LoadBool(b) => {
                    let _ = writeln!(out, "    stack[sp++] = trulang_val_bool({});", if *b { "true" } else { "false" });
                }
                Opcode::LoadNull => {
                    out.push_str("    stack[sp++] = trulang_val_null();\n");
                }
                Opcode::Pop => {
                    out.push_str("    if (sp > 0) sp--;\n");
                }
                Opcode::Dup => {
                    out.push_str("    stack[sp] = stack[sp - 1]; sp++;\n");
                }
                Opcode::GetLocal(idx) => {
                    let _ = writeln!(out, "    stack[sp++] = locals[{}];", idx);
                }
                Opcode::SetLocal(idx) => {
                    let _ = writeln!(out, "    locals[{}] = stack[--sp];", idx);
                }
                Opcode::GetField(idx) => {
                    let _ = writeln!(out, "    stack[sp++] = fields[{}];", idx);
                }
                Opcode::SetField(idx) => {
                    let _ = writeln!(out, "    fields[{}] = stack[--sp];", idx);
                }
                Opcode::GetFieldNamed(name) => {
                    let _ = writeln!(out, "    stack[sp - 1] = trulang_map_get(stack[sp - 1], \"{}\");", escape_c_string(name));
                }
                Opcode::SetFieldNamed(name) => {
                    let _ = writeln!(out, "    trulang_map_set(stack[sp - 2], \"{}\", stack[sp - 1]); sp -= 2;", escape_c_string(name));
                }
                Opcode::NewMap => {
                    out.push_str("    stack[sp++] = trulang_val_map(false);\n");
                }
                Opcode::CallMethod(name, arity) => {
                    if name == "json" {
                        // Response.json(arg) -> sets up response object
                        let _ = writeln!(out, "    // Response.json with arity {}", arity);
                        out.push_str("    // Keep the json payload on stack\n");
                    } else {
                        let _ = writeln!(out, "    // Method call '{}' with arity {}", name, arity);
                    }
                }
                Opcode::Add => {
                    out.push_str("    stack[sp - 2] = trulang_op_add(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Sub => {
                    out.push_str("    stack[sp - 2] = trulang_op_sub(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Mul => {
                    out.push_str("    stack[sp - 2] = trulang_op_mul(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Div => {
                    out.push_str("    stack[sp - 2] = trulang_op_div(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Mod => {
                    out.push_str("    stack[sp - 2] = trulang_op_mod(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Neg => {
                    out.push_str("    stack[sp - 1] = trulang_op_neg(stack[sp - 1]);\n");
                }
                Opcode::Not => {
                    out.push_str("    stack[sp - 1] = trulang_op_not(stack[sp - 1]);\n");
                }
                Opcode::Eq => {
                    out.push_str("    stack[sp - 2] = trulang_op_eq(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::NotEq => {
                    out.push_str("    stack[sp - 2] = trulang_op_ne(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Lt => {
                    out.push_str("    stack[sp - 2] = trulang_op_lt(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::LtEq => {
                    out.push_str("    stack[sp - 2] = trulang_op_le(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Gt => {
                    out.push_str("    stack[sp - 2] = trulang_op_gt(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::GtEq => {
                    out.push_str("    stack[sp - 2] = trulang_op_ge(stack[sp - 2], stack[sp - 1]); sp--;\n");
                }
                Opcode::Jump(target) => {
                    let _ = writeln!(out, "    goto label_{:04};", target);
                }
                Opcode::JumpIfFalse(target) => {
                    let _ = writeln!(out, "    if (!trulang_is_truthy(stack[--sp])) goto label_{:04};", target);
                }
                Opcode::Call(_arity) => {
                    // Function calls
                    out.push_str("    // Function call dispatch\n");
                }
                Opcode::Return => {
                    out.push_str("    if (sp > 0) { result_val = stack[--sp]; }\n");
                    out.push_str("    goto trulang_exit;\n");
                }
                Opcode::PushTry(catch_addr) => {
                    let _ = writeln!(out, "    // PushTry -> target catch at label_{:04}", catch_addr);
                }
                Opcode::PopTry => {
                    out.push_str("    // PopTry\n");
                }
                Opcode::Throw => {
                    out.push_str("    fprintf(stderr, \"\\nVM Runtime Error: Exception thrown\\n\");\n");
                    out.push_str("    exit(1);\n");
                }
                Opcode::PushDefer(addr) => {
                    let _ = writeln!(out, "    // PushDefer -> target at label_{:04}", addr);
                }
                Opcode::RunDefers => {
                    out.push_str("    // RunDefers\n");
                }
                Opcode::AllocRegionA(budget, max_b) => {
                    let max_val = max_b.unwrap_or(0);
                    let _ = writeln!(out, "    trulang_region_a_init({}, {});", budget, max_val);
                }
                Opcode::FreeRegionA => {
                    out.push_str("    trulang_region_a_reset();\n");
                }
                Opcode::Persist => {
                    out.push_str("    stack[sp - 1] = trulang_persist(stack[sp - 1]);\n");
                }
                Opcode::Print => {
                    out.push_str("    trulang_val_print(stack[--sp]);\n");
                }
                Opcode::Println => {
                    out.push_str("    trulang_val_println(stack[--sp]);\n");
                }
            }
        }

        // Implicit exit if falling off end
        let _ = writeln!(out, "label_{:04}:", chunk.code.len());
        out.push_str("    if (sp > 0) { result_val = stack[--sp]; }\n\n");

        out.push_str("trulang_exit:\n");
        out.push_str("    printf(\"\\nProgram exited with value: \");\n");
        out.push_str("    trulang_val_println(result_val);\n");
        out.push_str("    trulang_print_diagnostics();\n");
        out.push_str("    return 0;\n");
        out.push_str("}\n");

        out
    }
}

fn escape_c_string(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use trulang_compiler::bytecode::Opcode;

    #[test]
    fn test_c_backend_emits_valid_c_structure() {
        let mut chunk = Chunk::new();
        chunk.emit(Opcode::LoadInt(42), 1);
        chunk.emit(Opcode::LoadInt(58), 2);
        chunk.emit(Opcode::Add, 3);
        chunk.emit(Opcode::Return, 4);

        let c_code = CBackend::emit(&chunk);
        assert!(c_code.contains("trulang_region_a_init"));
        assert!(c_code.contains("trulang_op_add"));
        assert!(c_code.contains("int main(int argc, char** argv)"));
        assert!(c_code.contains("label_0000:"));
        assert!(c_code.contains("trulang_exit:"));
    }
}
