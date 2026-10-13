//! Independent reference Z80 interpreter.
//!
//! The game itself runs as hand-translated Rust (`decomp.rs`) on top of the ALU helpers in
//! `z80.rs`. To verify that translation we need an oracle that executes the *original ROM
//! bytes*. This interpreter deliberately re-implements every flag computation on its own (it
//! only borrows the register file and bus trait from `z80.rs`) so that a bug in the shared
//! helpers cannot cancel out in a comparison.
//!
//! Covers the documented instruction set plus IXH/IXL and the DDCB/FDCB register-copy forms.
//! Not used at runtime during play — only by `verify.rs` and the `verify_decomp` tool.

use crate::z80::{Z80, Z80Bus};

const S: u8 = 0x80;
const Z: u8 = 0x40;
const Y: u8 = 0x20;
const H: u8 = 0x10;
const X: u8 = 0x08;
const PV: u8 = 0x04;
const N: u8 = 0x02;
const C: u8 = 0x01;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pfx {
    None,
    Ix,
    Iy,
}

fn parity(v: u8) -> bool {
    v.count_ones() % 2 == 0
}
fn sz53(v: u8) -> u8 {
    (v & (S | Y | X)) | if v == 0 { Z } else { 0 }
}
fn sz53p(v: u8) -> u8 {
    sz53(v) | if parity(v) { PV } else { 0 }
}

fn add8(a: u8, b: u8, cin: u8) -> (u8, u8) {
    let s = a as u16 + b as u16 + cin as u16;
    let r = s as u8;
    let mut f = sz53(r);
    if s > 0xFF {
        f |= C;
    }
    if (a ^ b ^ r) & 0x10 != 0 {
        f |= H;
    }
    if (a ^ r) & (b ^ r) & 0x80 != 0 {
        f |= PV;
    }
    (r, f)
}

fn sub8(a: u8, b: u8, cin: u8) -> (u8, u8) {
    let d = a as i16 - b as i16 - cin as i16;
    let r = d as u8;
    let mut f = sz53(r) | N;
    if d < 0 {
        f |= C;
    }
    if (a ^ b ^ r) & 0x10 != 0 {
        f |= H;
    }
    if (a ^ b) & (a ^ r) & 0x80 != 0 {
        f |= PV;
    }
    (r, f)
}

fn cond(f: u8, cc: u8) -> bool {
    match cc & 7 {
        0 => f & Z == 0,
        1 => f & Z != 0,
        2 => f & C == 0,
        3 => f & C != 0,
        4 => f & PV == 0,
        5 => f & PV != 0,
        6 => f & S == 0,
        _ => f & S != 0,
    }
}

struct M<'a, B: Z80Bus> {
    c: &'a mut Z80,
    b: &'a mut B,
}

impl<'a, B: Z80Bus> M<'a, B> {
    fn rd(&mut self, a: u16) -> u8 {
        self.b.read_byte(self.c.cpu_id, a)
    }
    fn wr(&mut self, a: u16, v: u8) {
        self.b.write_byte(self.c.cpu_id, a, v)
    }
    fn fetch(&mut self) -> u8 {
        let v = self.rd(self.c.pc);
        self.c.pc = self.c.pc.wrapping_add(1);
        v
    }
    fn fetch16(&mut self) -> u16 {
        let lo = self.fetch() as u16;
        let hi = self.fetch() as u16;
        (hi << 8) | lo
    }
    fn rd16(&mut self, a: u16) -> u16 {
        let lo = self.rd(a) as u16;
        let hi = self.rd(a.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
    fn wr16(&mut self, a: u16, v: u16) {
        self.wr(a, v as u8);
        self.wr(a.wrapping_add(1), (v >> 8) as u8);
    }
    fn push(&mut self, v: u16) {
        self.c.sp = self.c.sp.wrapping_sub(1);
        let sp = self.c.sp;
        self.wr(sp, (v >> 8) as u8);
        self.c.sp = self.c.sp.wrapping_sub(1);
        let sp = self.c.sp;
        self.wr(sp, v as u8);
    }
    fn pop(&mut self) -> u16 {
        let sp = self.c.sp;
        let v = self.rd16(sp);
        self.c.sp = sp.wrapping_add(2);
        v
    }

    fn hlp(&self, p: Pfx) -> u16 {
        match p {
            Pfx::None => ((self.c.h as u16) << 8) | self.c.l as u16,
            Pfx::Ix => self.c.ix,
            Pfx::Iy => self.c.iy,
        }
    }
    fn set_hlp(&mut self, p: Pfx, v: u16) {
        match p {
            Pfx::None => {
                self.c.h = (v >> 8) as u8;
                self.c.l = v as u8;
            }
            Pfx::Ix => self.c.ix = v,
            Pfx::Iy => self.c.iy = v,
        }
    }
    /// Address of the `(HL)` / `(IX+d)` / `(IY+d)` operand (fetches the displacement if indexed).
    fn mem_addr(&mut self, p: Pfx) -> u16 {
        match p {
            Pfx::None => self.hlp(Pfx::None),
            Pfx::Ix => {
                let d = self.fetch() as i8;
                self.c.ix.wrapping_add(d as i16 as u16)
            }
            Pfx::Iy => {
                let d = self.fetch() as i8;
                self.c.iy.wrapping_add(d as i16 as u16)
            }
        }
    }
    /// 8-bit register by index (never 6). With an index prefix, H/L become IXH/IXL (IYH/IYL).
    fn get_r(&self, r: u8, p: Pfx) -> u8 {
        match r & 7 {
            0 => self.c.b,
            1 => self.c.c,
            2 => self.c.d,
            3 => self.c.e,
            4 => match p {
                Pfx::None => self.c.h,
                Pfx::Ix => (self.c.ix >> 8) as u8,
                Pfx::Iy => (self.c.iy >> 8) as u8,
            },
            5 => match p {
                Pfx::None => self.c.l,
                Pfx::Ix => self.c.ix as u8,
                Pfx::Iy => self.c.iy as u8,
            },
            _ => self.c.a,
        }
    }
    fn set_r(&mut self, r: u8, p: Pfx, v: u8) {
        match r & 7 {
            0 => self.c.b = v,
            1 => self.c.c = v,
            2 => self.c.d = v,
            3 => self.c.e = v,
            4 => match p {
                Pfx::None => self.c.h = v,
                Pfx::Ix => self.c.ix = (self.c.ix & 0x00FF) | ((v as u16) << 8),
                Pfx::Iy => self.c.iy = (self.c.iy & 0x00FF) | ((v as u16) << 8),
            },
            5 => match p {
                Pfx::None => self.c.l = v,
                Pfx::Ix => self.c.ix = (self.c.ix & 0xFF00) | v as u16,
                Pfx::Iy => self.c.iy = (self.c.iy & 0xFF00) | v as u16,
            },
            _ => self.c.a = v,
        }
    }
    fn rp(&self, p: u8, pfx: Pfx) -> u16 {
        match p & 3 {
            0 => ((self.c.b as u16) << 8) | self.c.c as u16,
            1 => ((self.c.d as u16) << 8) | self.c.e as u16,
            2 => self.hlp(pfx),
            _ => self.c.sp,
        }
    }
    fn set_rp(&mut self, p: u8, pfx: Pfx, v: u16) {
        match p & 3 {
            0 => {
                self.c.b = (v >> 8) as u8;
                self.c.c = v as u8;
            }
            1 => {
                self.c.d = (v >> 8) as u8;
                self.c.e = v as u8;
            }
            2 => self.set_hlp(pfx, v),
            _ => self.c.sp = v,
        }
    }
    fn rp2(&self, p: u8, pfx: Pfx) -> u16 {
        match p & 3 {
            3 => ((self.c.a as u16) << 8) | self.c.f as u16,
            q => self.rp(q, pfx),
        }
    }
    fn set_rp2(&mut self, p: u8, pfx: Pfx, v: u16) {
        match p & 3 {
            3 => {
                self.c.a = (v >> 8) as u8;
                self.c.f = v as u8;
            }
            q => self.set_rp(q, pfx, v),
        }
    }

    fn alu(&mut self, op: u8, v: u8) {
        let a = self.c.a;
        let cin = self.c.f & C;
        match op & 7 {
            0 => {
                let (r, f) = add8(a, v, 0);
                self.c.a = r;
                self.c.f = f;
            }
            1 => {
                let (r, f) = add8(a, v, cin);
                self.c.a = r;
                self.c.f = f;
            }
            2 => {
                let (r, f) = sub8(a, v, 0);
                self.c.a = r;
                self.c.f = f;
            }
            3 => {
                let (r, f) = sub8(a, v, cin);
                self.c.a = r;
                self.c.f = f;
            }
            4 => {
                let r = a & v;
                self.c.a = r;
                self.c.f = sz53p(r) | H;
            }
            5 => {
                let r = a ^ v;
                self.c.a = r;
                self.c.f = sz53p(r);
            }
            6 => {
                let r = a | v;
                self.c.a = r;
                self.c.f = sz53p(r);
            }
            _ => {
                let (_, f) = sub8(a, v, 0);
                self.c.f = (f & !(X | Y)) | (v & (X | Y));
            }
        }
    }

    fn inc8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_add(1);
        let mut f = (self.c.f & C) | sz53(r);
        if v & 0x0F == 0x0F {
            f |= H;
        }
        if v == 0x7F {
            f |= PV;
        }
        self.c.f = f;
        r
    }
    fn dec8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_sub(1);
        let mut f = (self.c.f & C) | sz53(r) | N;
        if v & 0x0F == 0 {
            f |= H;
        }
        if v == 0x80 {
            f |= PV;
        }
        self.c.f = f;
        r
    }

    fn add16(&mut self, a: u16, b: u16) -> u16 {
        let s = a as u32 + b as u32;
        let r = s as u16;
        let mut f = self.c.f & (S | Z | PV);
        f |= ((r >> 8) as u8) & (X | Y);
        if s > 0xFFFF {
            f |= C;
        }
        if (a ^ b ^ r) & 0x1000 != 0 {
            f |= H;
        }
        self.c.f = f;
        r
    }
    fn adc16(&mut self, a: u16, b: u16) -> u16 {
        let cin = (self.c.f & C) as u32;
        let s = a as u32 + b as u32 + cin;
        let r = s as u16;
        let mut f = ((r >> 8) as u8) & (S | X | Y);
        if r == 0 {
            f |= Z;
        }
        if s > 0xFFFF {
            f |= C;
        }
        if (a ^ b ^ r) & 0x1000 != 0 {
            f |= H;
        }
        if (a ^ r) & (b ^ r) & 0x8000 != 0 {
            f |= PV;
        }
        self.c.f = f;
        r
    }
    fn sbc16(&mut self, a: u16, b: u16) -> u16 {
        let cin = (self.c.f & C) as i32;
        let d = a as i32 - b as i32 - cin;
        let r = d as u16;
        let mut f = (((r >> 8) as u8) & (S | X | Y)) | N;
        if r == 0 {
            f |= Z;
        }
        if d < 0 {
            f |= C;
        }
        if (a ^ b ^ r) & 0x1000 != 0 {
            f |= H;
        }
        if (a ^ b) & (a ^ r) & 0x8000 != 0 {
            f |= PV;
        }
        self.c.f = f;
        r
    }

    /// CB-page rotate/shift `y` on `v`; sets flags.
    fn rot(&mut self, y: u8, v: u8) -> u8 {
        let (r, c) = match y & 7 {
            0 => (v.rotate_left(1), v >> 7),
            1 => (v.rotate_right(1), v & 1),
            2 => ((v << 1) | (self.c.f & C), v >> 7),
            3 => ((v >> 1) | ((self.c.f & C) << 7), v & 1),
            4 => (v << 1, v >> 7),
            5 => (((v as i8) >> 1) as u8, v & 1),
            6 => ((v << 1) | 1, v >> 7),
            _ => (v >> 1, v & 1),
        };
        self.c.f = sz53p(r) | c;
        r
    }
    fn bit(&mut self, y: u8, v: u8) {
        let set = v & (1 << y) != 0;
        let mut f = (self.c.f & C) | H | (v & (X | Y));
        if !set {
            f |= Z | PV;
        }
        if y == 7 && set {
            f |= S;
        }
        self.c.f = f;
    }

    fn daa(&mut self) {
        let a = self.c.a;
        let f = self.c.f;
        let mut corr = 0u8;
        let mut cf = f & C != 0;
        if f & H != 0 || (a & 0x0F) > 9 {
            corr |= 0x06;
        }
        if cf || a > 0x99 {
            corr |= 0x60;
            cf = true;
        }
        let (na, h) = if f & N != 0 {
            (a.wrapping_sub(corr), f & H != 0 && (a & 0x0F) < 6)
        } else {
            (a.wrapping_add(corr), (a & 0x0F) > 9)
        };
        self.c.a = na;
        self.c.f = sz53p(na) | (f & N) | if cf { C } else { 0 } | if h { H } else { 0 };
    }

    fn exec(&mut self) -> Result<(), String> {
        let start = self.c.pc;
        let mut pfx = Pfx::None;
        let mut op = self.fetch();
        loop {
            match op {
                0xDD => {
                    pfx = Pfx::Ix;
                    op = self.fetch();
                }
                0xFD => {
                    pfx = Pfx::Iy;
                    op = self.fetch();
                }
                _ => break,
            }
        }
        if op == 0xED {
            return self.exec_ed(start);
        }
        if op == 0xCB {
            return self.exec_cb(pfx);
        }
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let p = y >> 1;
        let q = y & 1;
        match x {
            0 => match z {
                0 => match y {
                    0 => {}
                    1 => {
                        std::mem::swap(&mut self.c.a, &mut self.c.a_alt);
                        std::mem::swap(&mut self.c.f, &mut self.c.f_alt);
                    }
                    2 => {
                        let d = self.fetch() as i8;
                        self.c.b = self.c.b.wrapping_sub(1);
                        if self.c.b != 0 {
                            self.c.pc = self.c.pc.wrapping_add(d as i16 as u16);
                        }
                    }
                    3 => {
                        let d = self.fetch() as i8;
                        self.c.pc = self.c.pc.wrapping_add(d as i16 as u16);
                    }
                    _ => {
                        let d = self.fetch() as i8;
                        if cond(self.c.f, y - 4) {
                            self.c.pc = self.c.pc.wrapping_add(d as i16 as u16);
                        }
                    }
                },
                1 => {
                    if q == 0 {
                        let v = self.fetch16();
                        self.set_rp(p, pfx, v);
                    } else {
                        let hl = self.hlp(pfx);
                        let rr = self.rp(p, pfx);
                        let r = self.add16(hl, rr);
                        self.set_hlp(pfx, r);
                    }
                }
                2 => match (q, p) {
                    (0, 0) => {
                        let a = self.rp(0, pfx);
                        let v = self.c.a;
                        self.wr(a, v);
                    }
                    (0, 1) => {
                        let a = self.rp(1, pfx);
                        let v = self.c.a;
                        self.wr(a, v);
                    }
                    (0, 2) => {
                        let a = self.fetch16();
                        let v = self.hlp(pfx);
                        self.wr16(a, v);
                    }
                    (0, _) => {
                        let a = self.fetch16();
                        let v = self.c.a;
                        self.wr(a, v);
                    }
                    (_, 0) => {
                        let a = self.rp(0, pfx);
                        self.c.a = self.rd(a);
                    }
                    (_, 1) => {
                        let a = self.rp(1, pfx);
                        self.c.a = self.rd(a);
                    }
                    (_, 2) => {
                        let a = self.fetch16();
                        let v = self.rd16(a);
                        self.set_hlp(pfx, v);
                    }
                    _ => {
                        let a = self.fetch16();
                        self.c.a = self.rd(a);
                    }
                },
                3 => {
                    let v = self.rp(p, pfx);
                    let v = if q == 0 { v.wrapping_add(1) } else { v.wrapping_sub(1) };
                    self.set_rp(p, pfx, v);
                }
                4 | 5 => {
                    if y == 6 {
                        let a = self.mem_addr(pfx);
                        let v = self.rd(a);
                        let r = if z == 4 { self.inc8(v) } else { self.dec8(v) };
                        self.wr(a, r);
                    } else {
                        let v = self.get_r(y, pfx);
                        let r = if z == 4 { self.inc8(v) } else { self.dec8(v) };
                        self.set_r(y, pfx, r);
                    }
                }
                6 => {
                    if y == 6 {
                        let a = self.mem_addr(pfx);
                        let n = self.fetch();
                        self.wr(a, n);
                    } else {
                        let n = self.fetch();
                        self.set_r(y, pfx, n);
                    }
                }
                _ => match y {
                    0 => {
                        let a = self.c.a;
                        let c = a >> 7;
                        let r = (a << 1) | c;
                        self.c.a = r;
                        self.c.f = (self.c.f & (S | Z | PV)) | (r & (X | Y)) | c;
                    }
                    1 => {
                        let a = self.c.a;
                        let c = a & 1;
                        let r = (a >> 1) | (c << 7);
                        self.c.a = r;
                        self.c.f = (self.c.f & (S | Z | PV)) | (r & (X | Y)) | c;
                    }
                    2 => {
                        let a = self.c.a;
                        let c = a >> 7;
                        let r = (a << 1) | (self.c.f & C);
                        self.c.a = r;
                        self.c.f = (self.c.f & (S | Z | PV)) | (r & (X | Y)) | c;
                    }
                    3 => {
                        let a = self.c.a;
                        let c = a & 1;
                        let r = (a >> 1) | ((self.c.f & C) << 7);
                        self.c.a = r;
                        self.c.f = (self.c.f & (S | Z | PV)) | (r & (X | Y)) | c;
                    }
                    4 => self.daa(),
                    5 => {
                        let r = !self.c.a;
                        self.c.a = r;
                        self.c.f = (self.c.f & (S | Z | PV | C)) | (r & (X | Y)) | H | N;
                    }
                    6 => {
                        self.c.f = (self.c.f & (S | Z | PV)) | (self.c.a & (X | Y)) | C;
                    }
                    _ => {
                        let oc = self.c.f & C;
                        self.c.f = (self.c.f & (S | Z | PV))
                            | (self.c.a & (X | Y))
                            | if oc != 0 { H } else { 0 }
                            | if oc != 0 { 0 } else { C };
                    }
                },
            },
            1 => {
                if y == 6 && z == 6 {
                    self.c.halted = true;
                } else if y == 6 {
                    let a = self.mem_addr(pfx);
                    let v = self.get_r(z, Pfx::None);
                    self.wr(a, v);
                } else if z == 6 {
                    let a = self.mem_addr(pfx);
                    let v = self.rd(a);
                    self.set_r(y, Pfx::None, v);
                } else {
                    let v = self.get_r(z, pfx);
                    self.set_r(y, pfx, v);
                }
            }
            2 => {
                let v = if z == 6 {
                    let a = self.mem_addr(pfx);
                    self.rd(a)
                } else {
                    self.get_r(z, pfx)
                };
                self.alu(y, v);
            }
            _ => match z {
                0 => {
                    if cond(self.c.f, y) {
                        self.c.pc = self.pop();
                    }
                }
                1 => {
                    if q == 0 {
                        let v = self.pop();
                        self.set_rp2(p, pfx, v);
                    } else {
                        match p {
                            0 => self.c.pc = self.pop(),
                            1 => {
                                std::mem::swap(&mut self.c.b, &mut self.c.b_alt);
                                std::mem::swap(&mut self.c.c, &mut self.c.c_alt);
                                std::mem::swap(&mut self.c.d, &mut self.c.d_alt);
                                std::mem::swap(&mut self.c.e, &mut self.c.e_alt);
                                std::mem::swap(&mut self.c.h, &mut self.c.h_alt);
                                std::mem::swap(&mut self.c.l, &mut self.c.l_alt);
                            }
                            2 => self.c.pc = self.hlp(pfx),
                            _ => self.c.sp = self.hlp(pfx),
                        }
                    }
                }
                2 => {
                    let nn = self.fetch16();
                    if cond(self.c.f, y) {
                        self.c.pc = nn;
                    }
                }
                3 => match y {
                    0 => self.c.pc = self.fetch16(),
                    2 => {
                        let n = self.fetch();
                        let port = ((self.c.a as u16) << 8) | n as u16;
                        let v = self.c.a;
                        self.b.write_port(self.c.cpu_id, port, v);
                    }
                    3 => {
                        let n = self.fetch();
                        let port = ((self.c.a as u16) << 8) | n as u16;
                        self.c.a = self.b.read_port(self.c.cpu_id, port);
                    }
                    4 => {
                        let sp = self.c.sp;
                        let m = self.rd16(sp);
                        let hl = self.hlp(pfx);
                        self.wr16(sp, hl);
                        self.set_hlp(pfx, m);
                    }
                    5 => {
                        std::mem::swap(&mut self.c.d, &mut self.c.h);
                        std::mem::swap(&mut self.c.e, &mut self.c.l);
                    }
                    6 => {
                        self.c.iff1 = false;
                        self.c.iff2 = false;
                    }
                    7 => {
                        self.c.iff1 = true;
                        self.c.iff2 = true;
                    }
                    _ => unreachable!(),
                },
                4 => {
                    let nn = self.fetch16();
                    if cond(self.c.f, y) {
                        let pc = self.c.pc;
                        self.push(pc);
                        self.c.pc = nn;
                    }
                }
                5 => {
                    if q == 0 {
                        let v = self.rp2(p, pfx);
                        self.push(v);
                    } else {
                        let nn = self.fetch16();
                        let pc = self.c.pc;
                        self.push(pc);
                        self.c.pc = nn;
                    }
                }
                6 => {
                    let n = self.fetch();
                    self.alu(y, n);
                }
                _ => {
                    let pc = self.c.pc;
                    self.push(pc);
                    self.c.pc = (y as u16) * 8;
                }
            },
        }
        Ok(())
    }

    fn exec_cb(&mut self, pfx: Pfx) -> Result<(), String> {
        if pfx == Pfx::None {
            let op = self.fetch();
            let (x, y, z) = (op >> 6, (op >> 3) & 7, op & 7);
            let hl = self.hlp(Pfx::None);
            let v = if z == 6 { self.rd(hl) } else { self.get_r(z, Pfx::None) };
            let res = match x {
                0 => Some(self.rot(y, v)),
                1 => {
                    self.bit(y, v);
                    None
                }
                2 => Some(v & !(1 << y)),
                _ => Some(v | (1 << y)),
            };
            if let Some(r) = res {
                if z == 6 {
                    self.wr(hl, r);
                } else {
                    self.set_r(z, Pfx::None, r);
                }
            }
        } else {
            let a = self.mem_addr(pfx);
            let op = self.fetch();
            let (x, y, z) = (op >> 6, (op >> 3) & 7, op & 7);
            let v = self.rd(a);
            let res = match x {
                0 => Some(self.rot(y, v)),
                1 => {
                    self.bit(y, v);
                    None
                }
                2 => Some(v & !(1 << y)),
                _ => Some(v | (1 << y)),
            };
            if let Some(r) = res {
                self.wr(a, r);
                if z != 6 {
                    self.set_r(z, Pfx::None, r);
                }
            }
        }
        Ok(())
    }

    fn exec_ed(&mut self, start: u16) -> Result<(), String> {
        let op = self.fetch();
        let (x, y, z) = (op >> 6, (op >> 3) & 7, op & 7);
        let (p, q) = (y >> 1, y & 1);
        match x {
            1 => match z {
                0 => {
                    let port = ((self.c.b as u16) << 8) | self.c.c as u16;
                    let v = self.b.read_port(self.c.cpu_id, port);
                    self.c.f = (self.c.f & C) | sz53p(v);
                    if y != 6 {
                        self.set_r(y, Pfx::None, v);
                    }
                }
                1 => {
                    let port = ((self.c.b as u16) << 8) | self.c.c as u16;
                    let v = if y == 6 { 0 } else { self.get_r(y, Pfx::None) };
                    self.b.write_port(self.c.cpu_id, port, v);
                }
                2 => {
                    let hl = self.hlp(Pfx::None);
                    let rr = self.rp(p, Pfx::None);
                    let r = if q == 0 { self.sbc16(hl, rr) } else { self.adc16(hl, rr) };
                    self.set_hlp(Pfx::None, r);
                }
                3 => {
                    let a = self.fetch16();
                    if q == 0 {
                        let v = self.rp(p, Pfx::None);
                        self.wr16(a, v);
                    } else {
                        let v = self.rd16(a);
                        self.set_rp(p, Pfx::None, v);
                    }
                }
                4 => {
                    let (r, f) = sub8(0, self.c.a, 0);
                    self.c.a = r;
                    self.c.f = f;
                }
                5 => {
                    self.c.pc = self.pop();
                    self.c.iff1 = self.c.iff2;
                }
                6 => self.c.im = [0, 0, 1, 2, 0, 0, 1, 2][y as usize],
                _ => match y {
                    0 => self.c.i = self.c.a,
                    1 => self.c.r = self.c.a,
                    2 | 3 => {
                        let v = if y == 2 { self.c.i } else { self.c.r };
                        self.c.a = v;
                        self.c.f = (self.c.f & C)
                            | sz53(v)
                            | if self.c.iff2 { PV } else { 0 };
                    }
                    4 => {
                        let hl = self.hlp(Pfx::None);
                        let m = self.rd(hl);
                        let a = self.c.a;
                        let nm = (a << 4) | (m >> 4);
                        self.c.a = (a & 0xF0) | (m & 0x0F);
                        self.wr(hl, nm);
                        self.c.f = (self.c.f & C) | sz53p(self.c.a);
                    }
                    5 => {
                        let hl = self.hlp(Pfx::None);
                        let m = self.rd(hl);
                        let a = self.c.a;
                        let nm = (m << 4) | (a & 0x0F);
                        self.c.a = (a & 0xF0) | (m >> 4);
                        self.wr(hl, nm);
                        self.c.f = (self.c.f & C) | sz53p(self.c.a);
                    }
                    _ => {}
                },
            },
            2 if y >= 4 && z <= 1 => {
                // LDI/LDD/LDIR/LDDR (z=0) and CPI/CPD/CPIR/CPDR (z=1)
                let dec = y & 1 != 0;
                let repeat = y >= 6;
                loop {
                    let hl = self.hlp(Pfx::None);
                    let v = self.rd(hl);
                    let bc = self.rp(0, Pfx::None).wrapping_sub(1);
                    let step = |a: u16| if dec { a.wrapping_sub(1) } else { a.wrapping_add(1) };
                    if z == 0 {
                        let de = self.rp(1, Pfx::None);
                        self.wr(de, v);
                        self.set_rp(1, Pfx::None, step(de));
                        self.set_hlp(Pfx::None, step(hl));
                        self.set_rp(0, Pfx::None, bc);
                        self.c.f = (self.c.f & (S | Z | C)) | if bc != 0 { PV } else { 0 };
                        if !(repeat && bc != 0) {
                            break;
                        }
                    } else {
                        let (r, f) = sub8(self.c.a, v, 0);
                        self.set_hlp(Pfx::None, step(hl));
                        self.set_rp(0, Pfx::None, bc);
                        self.c.f = (self.c.f & C)
                            | (f & (S | Z | H))
                            | N
                            | if bc != 0 { PV } else { 0 };
                        if !(repeat && bc != 0 && r != 0) {
                            break;
                        }
                    }
                }
            }
            _ => {
                return Err(format!(
                    "unsupported ED opcode {:02X} at {:04X} (cpu {})",
                    op, start, self.c.cpu_id
                ))
            }
        }
        Ok(())
    }
}

/// Executes exactly one instruction at `cpu.pc` using the reference interpreter.
pub fn ref_step<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) -> Result<(), String> {
    M { c: cpu, b: bus }.exec()
}

/// Outcome of running the reference interpreter toward a target state.
pub enum RefRun {
    Reached(u64),
    Timeout,
    Halted,
    Unsupported(String),
}

/// Runs the reference interpreter from the current state until `pc == target_pc` and
/// `sp == target_sp` (checked after each instruction, so at least one instruction executes).
pub fn ref_run_until<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    target_pc: u16,
    target_sp: u16,
    max_instr: u64,
) -> RefRun {
    for n in 1..=max_instr {
        if let Err(e) = ref_step(cpu, bus) {
            return RefRun::Unsupported(e);
        }
        if cpu.pc == target_pc && cpu.sp == target_sp {
            return RefRun::Reached(n);
        }
        if cpu.halted {
            return RefRun::Halted;
        }
    }
    RefRun::Timeout
}
