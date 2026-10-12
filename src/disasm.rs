use crate::symbols::{resolve_code_symbol, resolve_mem_symbol};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct DecodedInst {
    pub addr: u16,
    pub bytes: Vec<u8>,
    pub mnemonic: String,
    pub call_target: Option<u16>,
    pub branch_target: Option<u16>,
    pub mem_ref: Option<u16>,
    pub is_ret: bool,
    pub is_unconditional_jump: bool,
}

const REG8: [&str; 8] = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];
const REG16_SP: [&str; 4] = ["BC", "DE", "HL", "SP"];
const REG16_AF: [&str; 4] = ["BC", "DE", "HL", "AF"];
const COND: [&str; 8] = ["NZ", "Z", "NC", "C", "PO", "PE", "P", "M"];
const ALU_OPS: [&str; 8] = ["ADD A,", "ADC A,", "SUB", "SBC A,", "AND", "XOR", "OR", "CP"];
const ROT_OPS: [&str; 8] = ["RLC", "RRC", "RL", "RR", "SLA", "SRA", "SLL", "SRL"];

fn fmt_addr(addr: u16) -> String {
    if let Some(sym) = resolve_mem_symbol(addr) {
        format!("${:04X} /* {} */", addr, sym)
    } else {
        format!("${:04X}", addr)
    }
}

fn fmt_code_target(cpu_id: usize, addr: u16) -> String {
    if let Some((sym, _)) = resolve_code_symbol(cpu_id, addr) {
        format!("${:04X} <{}>", addr, sym)
    } else {
        format!("${:04X}", addr)
    }
}

pub fn decode_one(rom: &[u8], pc: u16, cpu_id: usize) -> DecodedInst {
    let idx = pc as usize;
    let r = |off: usize| -> u8 { *rom.get(idx + off).unwrap_or(&0xFF) };
    let r16 = |off: usize| -> u16 { (r(off) as u16) | ((r(off + 1) as u16) << 8) };

    let op0 = r(0);
    let mut len = 1usize;
    let mut call_target = None;
    let mut branch_target = None;
    let mut mem_ref = None;
    let mut is_ret = false;
    let mut is_unconditional_jump = false;

    let mnemonic = match op0 {
        0xDD | 0xFD => {
            let ix_iy = if op0 == 0xDD { "IX" } else { "IY" };
            let op1 = r(1);
            len = 2;
            if op1 == 0xCB {
                let disp = r(2) as i8;
                let op3 = r(3);
                len = 4;
                let disp_str = if disp >= 0 {
                    format!("({}+${:02X})", ix_iy, disp)
                } else {
                    format!("({}-${:02X})", ix_iy, -(disp as i16))
                };
                let x = op3 >> 6;
                let y = ((op3 >> 3) & 7) as usize;
                match x {
                    0 => format!("{} {}", ROT_OPS[y], disp_str),
                    1 => format!("BIT {}, {}", y, disp_str),
                    2 => format!("RES {}, {}", y, disp_str),
                    _ => format!("SET {}, {}", y, disp_str),
                }
            } else {
                let disp_fn = |d: u8| -> String {
                    let s = d as i8;
                    if s >= 0 {
                        format!("({}+${:02X})", ix_iy, s)
                    } else {
                        format!("({}-${:02X})", ix_iy, -(s as i16))
                    }
                };
                match op1 {
                    0x09 => format!("ADD {}, BC", ix_iy),
                    0x19 => format!("ADD {}, DE", ix_iy),
                    0x21 => {
                        len = 4;
                        let imm = r16(2);
                        mem_ref = Some(imm);
                        format!("LD {}, {}", ix_iy, fmt_addr(imm))
                    }
                    0x22 => {
                        len = 4;
                        let imm = r16(2);
                        mem_ref = Some(imm);
                        format!("LD ({}), {}", fmt_addr(imm), ix_iy)
                    }
                    0x23 => format!("INC {}", ix_iy),
                    0x29 => format!("ADD {}, {}", ix_iy, ix_iy),
                    0x2A => {
                        len = 4;
                        let imm = r16(2);
                        mem_ref = Some(imm);
                        format!("LD {}, ({})", ix_iy, fmt_addr(imm))
                    }
                    0x2B => format!("DEC {}", ix_iy),
                    0x34 => {
                        len = 3;
                        format!("INC {}", disp_fn(r(2)))
                    }
                    0x35 => {
                        len = 3;
                        format!("DEC {}", disp_fn(r(2)))
                    }
                    0x36 => {
                        len = 4;
                        format!("LD {}, ${:02X}", disp_fn(r(2)), r(3))
                    }
                    0x39 => format!("ADD {}, SP", ix_iy),
                    0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x7E => {
                        len = 3;
                        let dst = REG8[((op1 >> 3) & 7) as usize];
                        format!("LD {}, {}", dst, disp_fn(r(2)))
                    }
                    0x70..=0x75 | 0x77 => {
                        len = 3;
                        let src = REG8[(op1 & 7) as usize];
                        format!("LD {}, {}", disp_fn(r(2)), src)
                    }
                    0x86 | 0x8E | 0x96 | 0x9E | 0xA6 | 0xAE | 0xB6 | 0xBE => {
                        len = 3;
                        let alu = ALU_OPS[((op1 >> 3) & 7) as usize];
                        format!("{} {}", alu, disp_fn(r(2)))
                    }
                    0xE1 => format!("POP {}", ix_iy),
                    0xE3 => format!("EX (SP), {}", ix_iy),
                    0xE5 => format!("PUSH {}", ix_iy),
                    0xE9 => {
                        is_unconditional_jump = true;
                        format!("JP ({})", ix_iy)
                    }
                    0xF9 => format!("LD SP, {}", ix_iy),
                    _ => format!("DB ${:02X}, ${:02X}", op0, op1),
                }
            }
        }
        0xCB => {
            len = 2;
            let op1 = r(1);
            let x = op1 >> 6;
            let y = ((op1 >> 3) & 7) as usize;
            let z = (op1 & 7) as usize;
            match x {
                0 => format!("{} {}", ROT_OPS[y], REG8[z]),
                1 => format!("BIT {}, {}", y, REG8[z]),
                2 => format!("RES {}, {}", y, REG8[z]),
                _ => format!("SET {}, {}", y, REG8[z]),
            }
        }
        0xED => {
            len = 2;
            let op1 = r(1);
            match op1 {
                0x42 | 0x52 | 0x62 | 0x72 => {
                    format!("SBC HL, {}", REG16_SP[((op1 >> 4) & 3) as usize])
                }
                0x4A | 0x5A | 0x6A | 0x7A => {
                    format!("ADC HL, {}", REG16_SP[((op1 >> 4) & 3) as usize])
                }
                0x43 | 0x53 | 0x63 | 0x73 => {
                    len = 4;
                    let imm = r16(2);
                    mem_ref = Some(imm);
                    format!(
                        "LD ({}), {}",
                        fmt_addr(imm),
                        REG16_SP[((op1 >> 4) & 3) as usize]
                    )
                }
                0x4B | 0x5B | 0x6B | 0x7B => {
                    len = 4;
                    let imm = r16(2);
                    mem_ref = Some(imm);
                    format!(
                        "LD {}, ({})",
                        REG16_SP[((op1 >> 4) & 3) as usize],
                        fmt_addr(imm)
                    )
                }
                0x44 => "NEG".to_string(),
                0x45 | 0x55 | 0x65 | 0x75 => {
                    is_ret = true;
                    "RETN".to_string()
                }
                0x4D => {
                    is_ret = true;
                    "RETI".to_string()
                }
                0x46 => "IM 0".to_string(),
                0x56 => "IM 1".to_string(),
                0x5E => "IM 2".to_string(),
                0x47 => "LD I, A".to_string(),
                0x4F => "LD R, A".to_string(),
                0x57 => "LD A, I".to_string(),
                0x5F => "LD A, R".to_string(),
                0x67 => "RRD".to_string(),
                0x6F => "RLD".to_string(),
                0xA0 => "LDI".to_string(),
                0xA1 => "CPI".to_string(),
                0xA8 => "LDD".to_string(),
                0xA9 => "CPD".to_string(),
                0xB0 => "LDIR".to_string(),
                0xB1 => "CPIR".to_string(),
                0xB8 => "LDDR".to_string(),
                0xB9 => "CPDR".to_string(),
                _ => format!("DB $ED, ${:02X}", op1),
            }
        }
        0x00 => "NOP".to_string(),
        0x01 | 0x11 | 0x21 | 0x31 => {
            len = 3;
            let imm = r16(1);
            mem_ref = Some(imm);
            format!(
                "LD {}, {}",
                REG16_SP[((op0 >> 4) & 3) as usize],
                fmt_addr(imm)
            )
        }
        0x02 => "LD (BC), A".to_string(),
        0x12 => "LD (DE), A".to_string(),
        0x22 => {
            len = 3;
            let imm = r16(1);
            mem_ref = Some(imm);
            format!("LD ({}), HL", fmt_addr(imm))
        }
        0x32 => {
            len = 3;
            let imm = r16(1);
            mem_ref = Some(imm);
            format!("LD ({}), A", fmt_addr(imm))
        }
        0x0A => "LD A, (BC)".to_string(),
        0x1A => "LD A, (DE)".to_string(),
        0x2A => {
            len = 3;
            let imm = r16(1);
            mem_ref = Some(imm);
            format!("LD HL, ({})", fmt_addr(imm))
        }
        0x3A => {
            len = 3;
            let imm = r16(1);
            mem_ref = Some(imm);
            format!("LD A, ({})", fmt_addr(imm))
        }
        0x03 | 0x13 | 0x23 | 0x33 => format!("INC {}", REG16_SP[((op0 >> 4) & 3) as usize]),
        0x0B | 0x1B | 0x2B | 0x3B => format!("DEC {}", REG16_SP[((op0 >> 4) & 3) as usize]),
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
            format!("INC {}", REG8[((op0 >> 3) & 7) as usize])
        }
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
            format!("DEC {}", REG8[((op0 >> 3) & 7) as usize])
        }
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
            len = 2;
            format!("LD {}, ${:02X}", REG8[((op0 >> 3) & 7) as usize], r(1))
        }
        0x07 => "RLCA".to_string(),
        0x0F => "RRCA".to_string(),
        0x17 => "RLA".to_string(),
        0x1F => "RRA".to_string(),
        0x08 => "EX AF, AF'".to_string(),
        0x09 | 0x19 | 0x29 | 0x39 => format!("ADD HL, {}", REG16_SP[((op0 >> 4) & 3) as usize]),
        0x10 => {
            len = 2;
            let rel = r(1) as i8;
            let tgt = pc.wrapping_add(2).wrapping_add(rel as u16);
            branch_target = Some(tgt);
            format!("DJNZ {}", fmt_code_target(cpu_id, tgt))
        }
        0x18 => {
            len = 2;
            let rel = r(1) as i8;
            let tgt = pc.wrapping_add(2).wrapping_add(rel as u16);
            branch_target = Some(tgt);
            is_unconditional_jump = true;
            format!("JR {}", fmt_code_target(cpu_id, tgt))
        }
        0x20 | 0x28 | 0x30 | 0x38 => {
            len = 2;
            let rel = r(1) as i8;
            let tgt = pc.wrapping_add(2).wrapping_add(rel as u16);
            branch_target = Some(tgt);
            let cc = COND[((op0 - 0x20) >> 3) as usize];
            format!("JR {}, {}", cc, fmt_code_target(cpu_id, tgt))
        }
        0x27 => "DAA".to_string(),
        0x2F => "CPL".to_string(),
        0x37 => "SCF".to_string(),
        0x3F => "CCF".to_string(),
        0x76 => "HALT".to_string(),
        0x40..=0x7F => {
            let dst = REG8[((op0 >> 3) & 7) as usize];
            let src = REG8[(op0 & 7) as usize];
            format!("LD {}, {}", dst, src)
        }
        0x80..=0xBF => {
            let alu = ALU_OPS[((op0 >> 3) & 7) as usize];
            let src = REG8[(op0 & 7) as usize];
            format!("{} {}", alu, src)
        }
        0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => {
            format!("RET {}", COND[((op0 >> 3) & 7) as usize])
        }
        0xC9 => {
            is_ret = true;
            "RET".to_string()
        }
        0xD9 => "EXX".to_string(),
        0xE9 => {
            is_unconditional_jump = true;
            "JP (HL)".to_string()
        }
        0xF9 => "LD SP, HL".to_string(),
        0xC1 | 0xD1 | 0xE1 | 0xF1 => format!("POP {}", REG16_AF[((op0 >> 4) & 3) as usize]),
        0xC5 | 0xD5 | 0xE5 | 0xF5 => format!("PUSH {}", REG16_AF[((op0 >> 4) & 3) as usize]),
        0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => {
            len = 3;
            let tgt = r16(1);
            branch_target = Some(tgt);
            format!(
                "JP {}, {}",
                COND[((op0 >> 3) & 7) as usize],
                fmt_code_target(cpu_id, tgt)
            )
        }
        0xC3 => {
            len = 3;
            let tgt = r16(1);
            branch_target = Some(tgt);
            is_unconditional_jump = true;
            format!("JP {}", fmt_code_target(cpu_id, tgt))
        }
        0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => {
            len = 3;
            let tgt = r16(1);
            call_target = Some(tgt);
            format!(
                "CALL {}, {}",
                COND[((op0 >> 3) & 7) as usize],
                fmt_code_target(cpu_id, tgt)
            )
        }
        0xCD => {
            len = 3;
            let tgt = r16(1);
            call_target = Some(tgt);
            format!("CALL {}", fmt_code_target(cpu_id, tgt))
        }
        0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => {
            len = 2;
            let alu = ALU_OPS[((op0 >> 3) & 7) as usize];
            format!("{} ${:02X}", alu, r(1))
        }
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
            let rst_addr = (op0 & 0x38) as u16;
            call_target = Some(rst_addr);
            format!("RST ${:02X}", rst_addr)
        }
        0xD3 => {
            len = 2;
            format!("OUT (${:02X}), A", r(1))
        }
        0xDB => {
            len = 2;
            format!("IN A, (${:02X})", r(1))
        }
        0xE3 => "EX (SP), HL".to_string(),
        0xEB => "EX DE, HL".to_string(),
        0xF3 => "DI".to_string(),
        0xFB => "EI".to_string(),
    };

    let bytes = (0..len).map(|i| r(i)).collect();
    DecodedInst {
        addr: pc,
        bytes,
        mnemonic,
        call_target,
        branch_target,
        mem_ref,
        is_ret,
        is_unconditional_jump,
    }
}

pub struct DisassemblyResult {
    pub instructions: BTreeMap<u16, DecodedInst>,
    pub subroutines: BTreeSet<u16>,
    pub call_graph: BTreeMap<u16, BTreeSet<u16>>,
    pub ram_accesses: BTreeMap<u16, usize>,
}

/// Disassembles a CPU ROM using recursive control-flow traversal from hardware entry points
/// plus linear sweep over valid code regions.
pub fn analyze_cpu_rom(rom: &[u8], rom_limit: u16, cpu_id: usize) -> DisassemblyResult {
    let mut instructions = BTreeMap::new();
    let mut subroutines = BTreeSet::new();
    let mut call_graph: BTreeMap<u16, BTreeSet<u16>> = BTreeMap::new();
    let mut ram_accesses: BTreeMap<u16, usize> = BTreeMap::new();

    // Seed entry points: Reset (0x0000), RST vectors (0x0008, 0x0010, 0x0018, 0x0028, 0x0038), NMI (0x0066)
    let mut worklist = vec![0x0000u16, 0x0008, 0x0010, 0x0018, 0x0028, 0x0038, 0x0066];
    if cpu_id == 1 {
        worklist.push(0x0068);
        worklist.push(0x0080);
    } else if cpu_id == 2 {
        worklist.push(0x0085);
    }

    for &ep in &worklist {
        subroutines.insert(ep);
    }

    let mut visited = BTreeSet::new();
    while let Some(mut pc) = worklist.pop() {
        while pc < rom_limit && !visited.contains(&pc) {
            visited.insert(pc);
            let inst = decode_one(rom, pc, cpu_id);
            let next_pc = pc.wrapping_add(inst.bytes.len() as u16);

            if let Some(m) = inst.mem_ref {
                if m >= 0x6800 {
                    *ram_accesses.entry(m).or_insert(0) += 1;
                }
            }
            if let Some(tgt) = inst.call_target {
                if tgt < rom_limit {
                    subroutines.insert(tgt);
                    call_graph.entry(tgt).or_default().insert(pc);
                    if !visited.contains(&tgt) {
                        worklist.push(tgt);
                    }
                }
            }
            if let Some(tgt) = inst.branch_target {
                if tgt < rom_limit && !visited.contains(&tgt) {
                    worklist.push(tgt);
                }
            }

            let stop = inst.is_ret || inst.is_unconditional_jump;
            // Notice RST $28 on Z80 often uses an inline pointer table right after the RST instruction
            let is_rst28 = inst.bytes.first() == Some(&0xEF);
            instructions.insert(pc, inst);
            if stop || is_rst28 {
                break;
            }
            pc = next_pc;
        }
    }

    DisassemblyResult {
        instructions,
        subroutines,
        call_graph,
        ram_accesses,
    }
}
