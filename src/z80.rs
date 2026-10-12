/// Z80 CPU Register File & ALU State for the Native Rust Dig Dug Decompilation.
/// No runtime Z80 opcode decoding or interpreter is used during gameplay — all 3 CPUs
/// execute translated Rust functions (`src/decomp.rs`) directly against this state.

pub trait Z80Bus {
    fn read_byte(&mut self, cpu_id: usize, addr: u16) -> u8;
    fn write_byte(&mut self, cpu_id: usize, addr: u16, val: u8);
    fn read_word(&mut self, cpu_id: usize, addr: u16) -> u16 {
        let lo = self.read_byte(cpu_id, addr) as u16;
        let hi = self.read_byte(cpu_id, addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
    fn write_word(&mut self, cpu_id: usize, addr: u16, val: u16) {
        self.write_byte(cpu_id, addr, (val & 0xFF) as u8);
        self.write_byte(cpu_id, addr.wrapping_add(1), (val >> 8) as u8);
    }
    fn read_port(&mut self, _cpu_id: usize, _port: u16) -> u8 {
        0xFF
    }
    fn write_port(&mut self, _cpu_id: usize, _port: u16, _val: u8) {}
}

pub const FLAG_C: u8 = 0x01;
pub const FLAG_N: u8 = 0x02;
pub const FLAG_PV: u8 = 0x04;
pub const FLAG_X: u8 = 0x08;
pub const FLAG_H: u8 = 0x10;
pub const FLAG_Y: u8 = 0x20;
pub const FLAG_Z: u8 = 0x40;
pub const FLAG_S: u8 = 0x80;

#[derive(Clone, Debug)]
pub struct Z80 {
    pub cpu_id: usize,
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,

    pub a_alt: u8,
    pub f_alt: u8,
    pub b_alt: u8,
    pub c_alt: u8,
    pub d_alt: u8,
    pub e_alt: u8,
    pub h_alt: u8,
    pub l_alt: u8,

    pub ix: u16,
    pub iy: u16,
    pub sp: u16,
    pub pc: u16,
    pub i: u8,
    pub r: u8,

    pub iff1: bool,
    pub iff2: bool,
    pub im: u8,
    pub halted: bool,
    pub total_cycles: u64,
    pub instructions_executed: u64,
}

impl Z80 {
    pub fn new(cpu_id: usize) -> Self {
        Self {
            cpu_id,
            a: 0xFF,
            f: 0xFF,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            a_alt: 0,
            f_alt: 0,
            b_alt: 0,
            c_alt: 0,
            d_alt: 0,
            e_alt: 0,
            h_alt: 0,
            l_alt: 0,
            ix: 0,
            iy: 0,
            sp: 0xFFFF,
            pc: 0x0000,
            i: 0,
            r: 0,
            iff1: false,
            iff2: false,
            im: 0,
            halted: false,
            total_cycles: 0,
            instructions_executed: 0,
        }
    }

    pub fn reset(&mut self) {
        self.pc = 0x0000;
        self.sp = 0xFFFF;
        self.i = 0;
        self.r = 0;
        self.iff1 = false;
        self.iff2 = false;
        self.im = 0;
        self.halted = false;
    }

    #[inline(always)]
    pub fn af(&self) -> u16 {
        ((self.a as u16) << 8) | (self.f as u16)
    }
    #[inline(always)]
    pub fn set_af(&mut self, v: u16) {
        self.a = (v >> 8) as u8;
        self.f = (v & 0xFF) as u8;
    }

    #[inline(always)]
    pub fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | (self.c as u16)
    }
    #[inline(always)]
    pub fn set_bc(&mut self, v: u16) {
        self.b = (v >> 8) as u8;
        self.c = (v & 0xFF) as u8;
    }

    #[inline(always)]
    pub fn de(&self) -> u16 {
        ((self.d as u16) << 8) | (self.e as u16)
    }
    #[inline(always)]
    pub fn set_de(&mut self, v: u16) {
        self.d = (v >> 8) as u8;
        self.e = (v & 0xFF) as u8;
    }

    #[inline(always)]
    pub fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | (self.l as u16)
    }
    #[inline(always)]
    pub fn set_hl(&mut self, v: u16) {
        self.h = (v >> 8) as u8;
        self.l = (v & 0xFF) as u8;
    }

    #[inline(always)]
    pub fn push<B: Z80Bus>(&mut self, bus: &mut B, val: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write_byte(self.cpu_id, self.sp, (val >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write_byte(self.cpu_id, self.sp, (val & 0xFF) as u8);
    }

    #[inline(always)]
    pub fn pop<B: Z80Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = bus.read_byte(self.cpu_id, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read_byte(self.cpu_id, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    #[inline(always)]
    pub fn ret_subroutine<B: Z80Bus>(&mut self, bus: &mut B) {
        self.pc = self.pop(bus);
    }

    pub fn trigger_nmi<B: Z80Bus>(&mut self, bus: &mut B) {
        self.halted = false;
        self.iff2 = self.iff1;
        self.iff1 = false;
        let pc = self.pc;
        self.push(bus, pc);
        self.pc = 0x0066;
        self.total_cycles += 11;
    }

    pub fn trigger_irq<B: Z80Bus>(&mut self, bus: &mut B) -> bool {
        if !self.iff1 {
            return false;
        }
        self.halted = false;
        self.iff1 = false;
        self.iff2 = false;
        let pc = self.pc;
        self.push(bus, pc);
        self.pc = 0x0038;
        self.total_cycles += 13;
        true
    }

    #[inline(always)]
    pub fn szp_flags(val: u8) -> u8 {
        let mut f = val & (FLAG_S | FLAG_X | FLAG_Y);
        if val == 0 {
            f |= FLAG_Z;
        }
        if val.count_ones() % 2 == 0 {
            f |= FLAG_PV;
        }
        f
    }

    #[inline(always)]
    pub fn check_cond(&self, cc: u8) -> bool {
        match cc & 7 {
            0 => (self.f & FLAG_Z) == 0,  // NZ
            1 => (self.f & FLAG_Z) != 0,  // Z
            2 => (self.f & FLAG_C) == 0,  // NC
            3 => (self.f & FLAG_C) != 0,  // C
            4 => (self.f & FLAG_PV) == 0, // PO
            5 => (self.f & FLAG_PV) != 0, // PE
            6 => (self.f & FLAG_S) == 0,  // P
            _ => (self.f & FLAG_S) != 0,  // M
        }
    }

    #[inline(always)]
    pub fn inc8(&mut self, v: u8) -> u8 {
        let res = v.wrapping_add(1);
        let mut f = self.f & FLAG_C;
        f |= res & (FLAG_S | FLAG_X | FLAG_Y);
        if res == 0 {
            f |= FLAG_Z;
        }
        if (v & 0x0F) == 0x0F {
            f |= FLAG_H;
        }
        if v == 0x7F {
            f |= FLAG_PV;
        }
        self.f = f;
        res
    }

    #[inline(always)]
    pub fn dec8(&mut self, v: u8) -> u8 {
        let res = v.wrapping_sub(1);
        let mut f = (self.f & FLAG_C) | FLAG_N;
        f |= res & (FLAG_S | FLAG_X | FLAG_Y);
        if res == 0 {
            f |= FLAG_Z;
        }
        if (v & 0x0F) == 0x00 {
            f |= FLAG_H;
        }
        if v == 0x80 {
            f |= FLAG_PV;
        }
        self.f = f;
        res
    }

    #[inline(always)]
    pub fn add8(&mut self, val: u8, carry: bool) {
        let c = if carry && (self.f & FLAG_C) != 0 { 1u16 } else { 0u16 };
        let a = self.a as u16;
        let b = val as u16;
        let sum = a + b + c;
        let res = (sum & 0xFF) as u8;
        let mut f = res & (FLAG_S | FLAG_X | FLAG_Y);
        if res == 0 {
            f |= FLAG_Z;
        }
        if sum > 0xFF {
            f |= FLAG_C;
        }
        if ((a ^ b ^ sum) & 0x10) != 0 {
            f |= FLAG_H;
        }
        if (((a ^ b ^ 0x80) & (a ^ sum)) & 0x80) != 0 {
            f |= FLAG_PV;
        }
        self.a = res;
        self.f = f;
    }

    #[inline(always)]
    pub fn sub8(&mut self, val: u8, carry: bool, store: bool) {
        let c = if carry && (self.f & FLAG_C) != 0 { 1u16 } else { 0u16 };
        let a = self.a as u16;
        let b = val as u16;
        let diff = a.wrapping_sub(b).wrapping_sub(c);
        let res = (diff & 0xFF) as u8;
        let mut f = FLAG_N | (res & (FLAG_S | FLAG_X | FLAG_Y));
        if res == 0 {
            f |= FLAG_Z;
        }
        if diff > 0xFF {
            f |= FLAG_C;
        }
        if ((a ^ b ^ diff) & 0x10) != 0 {
            f |= FLAG_H;
        }
        if (((a ^ b) & (a ^ diff)) & 0x80) != 0 {
            f |= FLAG_PV;
        }
        if store {
            self.a = res;
        }
        self.f = f;
    }

    #[inline(always)]
    pub fn and8(&mut self, val: u8) {
        self.a &= val;
        self.f = Self::szp_flags(self.a) | FLAG_H;
    }

    #[inline(always)]
    pub fn xor8(&mut self, val: u8) {
        self.a ^= val;
        self.f = Self::szp_flags(self.a);
    }

    #[inline(always)]
    pub fn or8(&mut self, val: u8) {
        self.a |= val;
        self.f = Self::szp_flags(self.a);
    }

    #[inline(always)]
    pub fn alu8(&mut self, op: u8, val: u8) {
        match op & 7 {
            0 => self.add8(val, false),
            1 => self.add8(val, true),
            2 => self.sub8(val, false, true),
            3 => self.sub8(val, true, true),
            4 => self.and8(val),
            5 => self.xor8(val),
            6 => self.or8(val),
            _ => self.sub8(val, false, false),
        }
    }

    #[inline(always)]
    pub fn daa(&mut self) {
        let mut a = self.a;
        let mut adjust = 0u8;
        let mut carry = (self.f & FLAG_C) != 0;
        if (self.f & FLAG_H) != 0 || ((self.f & FLAG_N) == 0 && (a & 0x0F) > 9) {
            adjust |= 0x06;
        }
        if carry || ((self.f & FLAG_N) == 0 && a > 0x99) {
            adjust |= 0x60;
            carry = true;
        }
        if (self.f & FLAG_N) != 0 {
            a = a.wrapping_sub(adjust);
        } else {
            a = a.wrapping_add(adjust);
        }
        self.a = a;
        self.f = Self::szp_flags(a)
            | (self.f & FLAG_N)
            | (if carry { FLAG_C } else { 0 });
    }

    #[inline(always)]
    pub fn add16(&mut self, lhs: u16, rhs: u16) -> u16 {
        let sum = (lhs as u32) + (rhs as u32);
        let res = (sum & 0xFFFF) as u16;
        let mut f = self.f & (FLAG_S | FLAG_Z | FLAG_PV);
        if sum > 0xFFFF {
            f |= FLAG_C;
        }
        if ((lhs ^ rhs ^ res) & 0x1000) != 0 {
            f |= FLAG_H;
        }
        self.f = f;
        res
    }

    #[inline(always)]
    pub fn sbc_hl(&mut self, rhs: u16) {
        let rhs = rhs as u32;
        let c = if (self.f & FLAG_C) != 0 { 1u32 } else { 0u32 };
        let lhs = self.hl() as u32;
        let diff = lhs.wrapping_sub(rhs).wrapping_sub(c);
        let res = (diff & 0xFFFF) as u16;
        let mut f = FLAG_N;
        if (res & 0x8000) != 0 {
            f |= FLAG_S;
        }
        if res == 0 {
            f |= FLAG_Z;
        }
        if diff > 0xFFFF {
            f |= FLAG_C;
        }
        if ((lhs ^ rhs ^ diff) & 0x1000) != 0 {
            f |= FLAG_H;
        }
        if (((lhs ^ rhs) & (lhs ^ diff)) & 0x8000) != 0 {
            f |= FLAG_PV;
        }
        self.set_hl(res);
        self.f = f;
    }

    #[inline(always)]
    pub fn adc_hl(&mut self, rhs: u16) {
        let rhs = rhs as u32;
        let c = if (self.f & FLAG_C) != 0 { 1u32 } else { 0u32 };
        let lhs = self.hl() as u32;
        let sum = lhs + rhs + c;
        let res = (sum & 0xFFFF) as u16;
        let mut f = 0;
        if (res & 0x8000) != 0 {
            f |= FLAG_S;
        }
        if res == 0 {
            f |= FLAG_Z;
        }
        if sum > 0xFFFF {
            f |= FLAG_C;
        }
        if ((lhs ^ rhs ^ sum) & 0x1000) != 0 {
            f |= FLAG_H;
        }
        if (((lhs ^ rhs ^ 0x8000) & (lhs ^ sum)) & 0x8000) != 0 {
            f |= FLAG_PV;
        }
        self.set_hl(res);
        self.f = f;
    }

    #[inline(always)]
    pub fn rrd<B: Z80Bus>(&mut self, bus: &mut B) {
        let hl = self.hl();
        let m = bus.read_byte(self.cpu_id, hl);
        let new_m = (self.a << 4) | (m >> 4);
        self.a = (self.a & 0xF0) | (m & 0x0F);
        bus.write_byte(self.cpu_id, hl, new_m);
        self.f = (self.f & FLAG_C) | Self::szp_flags(self.a);
    }

    #[inline(always)]
    pub fn rld<B: Z80Bus>(&mut self, bus: &mut B) {
        let hl = self.hl();
        let m = bus.read_byte(self.cpu_id, hl);
        let new_m = (m << 4) | (self.a & 0x0F);
        self.a = (self.a & 0xF0) | (m >> 4);
        bus.write_byte(self.cpu_id, hl, new_m);
        self.f = (self.f & FLAG_C) | Self::szp_flags(self.a);
    }

    #[inline(always)]
    pub fn ldi<B: Z80Bus>(&mut self, bus: &mut B) {
        let hl = self.hl();
        let de = self.de();
        let bc = self.bc().wrapping_sub(1);
        let val = bus.read_byte(self.cpu_id, hl);
        bus.write_byte(self.cpu_id, de, val);
        self.set_hl(hl.wrapping_add(1));
        self.set_de(de.wrapping_add(1));
        self.set_bc(bc);
        self.f = (self.f & (FLAG_S | FLAG_Z | FLAG_C)) | (if bc != 0 { FLAG_PV } else { 0 });
    }

    #[inline(always)]
    pub fn ldd<B: Z80Bus>(&mut self, bus: &mut B) {
        let hl = self.hl();
        let de = self.de();
        let bc = self.bc().wrapping_sub(1);
        let val = bus.read_byte(self.cpu_id, hl);
        bus.write_byte(self.cpu_id, de, val);
        self.set_hl(hl.wrapping_sub(1));
        self.set_de(de.wrapping_sub(1));
        self.set_bc(bc);
        self.f = (self.f & (FLAG_S | FLAG_Z | FLAG_C)) | (if bc != 0 { FLAG_PV } else { 0 });
    }

    #[inline(always)]
    pub fn ldir<B: Z80Bus>(&mut self, bus: &mut B, repeat_pc: u16) -> u32 {
        let hl = self.hl();
        let de = self.de();
        let bc = self.bc().wrapping_sub(1);
        let val = bus.read_byte(self.cpu_id, hl);
        bus.write_byte(self.cpu_id, de, val);
        self.set_hl(hl.wrapping_add(1));
        self.set_de(de.wrapping_add(1));
        self.set_bc(bc);
        if bc != 0 {
            self.f = (self.f & (FLAG_S | FLAG_Z | FLAG_C)) | FLAG_PV;
            self.pc = repeat_pc;
            21
        } else {
            self.f &= FLAG_S | FLAG_Z | FLAG_C;
            16
        }
    }

    #[inline(always)]
    pub fn lddr<B: Z80Bus>(&mut self, bus: &mut B, repeat_pc: u16) -> u32 {
        let hl = self.hl();
        let de = self.de();
        let bc = self.bc().wrapping_sub(1);
        let val = bus.read_byte(self.cpu_id, hl);
        bus.write_byte(self.cpu_id, de, val);
        self.set_hl(hl.wrapping_sub(1));
        self.set_de(de.wrapping_sub(1));
        self.set_bc(bc);
        if bc != 0 {
            self.f = (self.f & (FLAG_S | FLAG_Z | FLAG_C)) | FLAG_PV;
            self.pc = repeat_pc;
            21
        } else {
            self.f &= FLAG_S | FLAG_Z | FLAG_C;
            16
        }
    }

    #[inline(always)]
    pub fn cpi_cpd<B: Z80Bus>(&mut self, bus: &mut B, sub: u8, repeat_pc: u16) -> u32 {
        let hl = self.hl();
        let val = bus.read_byte(self.cpu_id, hl);
        let old_c = self.f & FLAG_C;
        self.sub8(val, false, false);
        if (sub & 0x08) != 0 {
            self.set_hl(hl.wrapping_sub(1));
        } else {
            self.set_hl(hl.wrapping_add(1));
        }
        let bc = self.bc().wrapping_sub(1);
        self.set_bc(bc);
        let z = (self.f & FLAG_Z) != 0;
        self.f = (self.f & (FLAG_S | FLAG_Z | FLAG_H))
            | FLAG_N
            | old_c
            | (if bc != 0 { FLAG_PV } else { 0 });
        if (sub & 0x10) != 0 && bc != 0 && !z {
            self.pc = repeat_pc;
            21
        } else {
            16
        }
    }

    #[inline(always)]
    pub fn cb_op(&mut self, op: u8, val: u8) -> u8 {
        let x = op >> 6;
        let y = (op >> 3) & 7;
        match x {
            0 => {
                let (res, c) = match y {
                    0 => (val.rotate_left(1), (val >> 7) & 1),  // RLC
                    1 => (val.rotate_right(1), val & 1),        // RRC
                    2 => {
                        // RL
                        let old_c = if (self.f & FLAG_C) != 0 { 1 } else { 0 };
                        ((val << 1) | old_c, (val >> 7) & 1)
                    }
                    3 => {
                        // RR
                        let old_c = if (self.f & FLAG_C) != 0 { 0x80 } else { 0 };
                        ((val >> 1) | old_c, val & 1)
                    }
                    4 => (val << 1, (val >> 7) & 1),        // SLA
                    5 => ((val as i8 >> 1) as u8, val & 1), // SRA
                    6 => ((val << 1) | 1, (val >> 7) & 1),  // SLL
                    _ => (val >> 1, val & 1),               // SRL
                };
                self.f = Self::szp_flags(res) | (if c != 0 { FLAG_C } else { 0 });
                res
            }
            1 => {
                // BIT y, val
                let bit = val & (1 << y);
                let mut f = (self.f & FLAG_C) | FLAG_H;
                if bit == 0 {
                    f |= FLAG_Z | FLAG_PV;
                } else if y == 7 {
                    f |= FLAG_S;
                }
                self.f = f;
                val
            }
            2 => val & !(1 << y), // RES y, val
            _ => val | (1 << y),  // SET y, val
        }
    }

    #[inline(always)]
    pub fn get_reg8<B: Z80Bus>(&self, bus: &mut B, r: u8) -> u8 {
        match r & 7 {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read_byte(self.cpu_id, self.hl()),
            _ => self.a,
        }
    }

    #[inline(always)]
    pub fn set_reg8<B: Z80Bus>(&mut self, bus: &mut B, r: u8, val: u8) {
        match r & 7 {
            0 => self.b = val,
            1 => self.c = val,
            2 => self.d = val,
            3 => self.e = val,
            4 => self.h = val,
            5 => self.l = val,
            6 => bus.write_byte(self.cpu_id, self.hl(), val),
            _ => self.a = val,
        }
    }

    #[inline(always)]
    pub fn get_reg16(&self, rp: u8) -> u16 {
        match rp & 3 {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hl(),
            _ => self.sp,
        }
    }

    #[inline(always)]
    pub fn set_reg16(&mut self, rp: u8, val: u16) {
        match rp & 3 {
            0 => self.set_bc(val),
            1 => self.set_de(val),
            2 => self.set_hl(val),
            _ => self.sp = val,
        }
    }
}
