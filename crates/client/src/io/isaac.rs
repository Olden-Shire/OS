// @ObfuscatedName("bl") — jag::oldscape::io::Isaac.
//
// Verbatim port of the ISAAC PRNG cipher. The post-login game packets
// XOR every byte with the next i32 from this stream (PacketBit.p1Enc /
// g1Enc); without it the protocol stays scrambled.
//
// Translation rules:
// - Java's `>>>` (unsigned right shift) maps to Rust's `as u32 >> n`,
//   then cast back to i32.
// - All wrapping arithmetic uses `wrapping_*`.

#![allow(dead_code)]

pub struct Isaac {
    // @ObfuscatedName("bl.m")
    pub count: i32,
    // @ObfuscatedName("bl.n")
    pub rsl: [i32; 256],
    // @ObfuscatedName("bl.j")
    pub mem: [i32; 256],
    // @ObfuscatedName("bl.z")
    pub a: i32,
    // @ObfuscatedName("bl.g")
    pub b: i32,
    // @ObfuscatedName("bl.q")
    pub c: i32,
}

#[inline]
fn shr(x: i32, n: u32) -> i32 { ((x as u32) >> n) as i32 }

impl Isaac {
    pub fn new(seed: &[i32]) -> Self {
        let mut rsl = [0i32; 256];
        for (i, &v) in seed.iter().enumerate().take(256) {
            rsl[i] = v;
        }
        let mut s = Self { count: 0, rsl, mem: [0i32; 256], a: 0, b: 0, c: 0 };
        s.init();
        s
    }

    // @ObfuscatedName("bl.r(I)I") — Isaac.takeNextValue.
    pub fn take_next_value(&mut self) -> i32 {
        self.count -= 1;
        if self.count + 1 == 0 {
            self.generate();
            self.count = 255;
        }
        self.rsl[self.count as usize]
    }

    // @ObfuscatedName("bl.d(I)V") — Isaac.generate. Per-256 cipher
    // step. Verbatim port of the Java source above.
    pub fn generate(&mut self) {
        self.c = self.c.wrapping_add(1);
        self.b = self.b.wrapping_add(self.c);
        for var1 in 0..256usize {
            let var2 = self.mem[var1];
            if (var1 & 0x2) == 0 {
                if (var1 & 0x1) == 0 {
                    self.a ^= self.a.wrapping_shl(13);
                } else {
                    self.a ^= shr(self.a, 6);
                }
            } else if (var1 & 0x1) == 0 {
                self.a ^= self.a.wrapping_shl(2);
            } else {
                self.a ^= shr(self.a, 16);
            }
            let i_plus_128 = (var1 + 128) & 0xFF;
            self.a = self.a.wrapping_add(self.mem[i_plus_128]);
            let idx_lo = (shr(var2, 2) & 0xFF) as usize;
            let var3 = self.b.wrapping_add(self.a).wrapping_add(self.mem[idx_lo]);
            self.mem[var1] = var3;
            let idx_hi = (shr(shr(var3, 8), 2) & 0xFF) as usize;
            self.b = self.mem[idx_hi].wrapping_add(var2);
            self.rsl[var1] = self.b;
        }
    }

    // @ObfuscatedName("bl.l(I)V") — Isaac.init. Golden-ratio seed
    // expansion + two mixing passes over rsl and mem.
    pub fn init(&mut self) {
        let mut var1: i32 = -1640531527;
        let mut var2 = var1;
        let mut var3 = var1;
        let mut var4 = var1;
        let mut var5 = var1;
        let mut var6 = var1;
        let mut var7 = var1;
        let mut var8 = var1;

        // 4 rounds of golden-ratio mixing.
        for _ in 0..4 {
            let var10 = var8 ^ var7.wrapping_shl(11);
            let var11 = var5.wrapping_add(var10);
            let var12 = var6.wrapping_add(var7);
            let var13 = var12 ^ shr(var6, 2);
            let var14 = var4.wrapping_add(var13);
            let var15 = var6.wrapping_add(var11);
            let var16 = var15 ^ var11.wrapping_shl(8);
            let var17 = var3.wrapping_add(var16);
            let var18 = var11.wrapping_add(var14);
            var5 = var18 ^ shr(var14, 16);
            let var19 = var2.wrapping_add(var5);
            let var20 = var14.wrapping_add(var17);
            var4 = var20 ^ var17.wrapping_shl(10);
            let var21 = var1.wrapping_add(var4);
            let var22 = var17.wrapping_add(var19);
            var3 = var22 ^ shr(var19, 4);
            let var23 = var3.wrapping_add(var10);
            let var24 = var19.wrapping_add(var21);
            var2 = var24 ^ var21.wrapping_shl(8);
            var7 = var2.wrapping_add(var13);
            let var25 = var21.wrapping_add(var23);
            var1 = var25 ^ shr(var23, 9);
            var6 = var1.wrapping_add(var16);
            var8 = var7.wrapping_add(var23);
        }

        // First pass: mix rsl[] into mem[] 8 at a time.
        let mut var26 = 0usize;
        while var26 < 256 {
            let var27 = self.rsl[var26].wrapping_add(var8);
            let var28 = self.rsl[var26 + 1].wrapping_add(var7);
            let var29 = self.rsl[var26 + 2].wrapping_add(var6);
            let var30 = self.rsl[var26 + 3].wrapping_add(var5);
            let var31 = self.rsl[var26 + 4].wrapping_add(var4);
            let var32 = self.rsl[var26 + 5].wrapping_add(var3);
            let var33 = self.rsl[var26 + 6].wrapping_add(var2);
            let var34 = self.rsl[var26 + 7].wrapping_add(var1);
            let var35 = var27 ^ var28.wrapping_shl(11);
            let var36 = var30.wrapping_add(var35);
            let var37 = var28.wrapping_add(var29);
            let var38 = var37 ^ shr(var29, 2);
            let var39 = var31.wrapping_add(var38);
            let var40 = var29.wrapping_add(var36);
            let var41 = var40 ^ var36.wrapping_shl(8);
            let var42 = var32.wrapping_add(var41);
            let var43 = var36.wrapping_add(var39);
            var5 = var43 ^ shr(var39, 16);
            let var44 = var5.wrapping_add(var33);
            let var45 = var39.wrapping_add(var42);
            var4 = var45 ^ var42.wrapping_shl(10);
            let var46 = var4.wrapping_add(var34);
            let var47 = var42.wrapping_add(var44);
            var3 = var47 ^ shr(var44, 4);
            let var48 = var3.wrapping_add(var35);
            let var49 = var44.wrapping_add(var46);
            var2 = var49 ^ var46.wrapping_shl(8);
            var7 = var2.wrapping_add(var38);
            let var50 = var46.wrapping_add(var48);
            var1 = var50 ^ shr(var48, 9);
            var6 = var1.wrapping_add(var41);
            var8 = var7.wrapping_add(var48);
            self.mem[var26] = var8;
            self.mem[var26 + 1] = var7;
            self.mem[var26 + 2] = var6;
            self.mem[var26 + 3] = var5;
            self.mem[var26 + 4] = var4;
            self.mem[var26 + 5] = var3;
            self.mem[var26 + 6] = var2;
            self.mem[var26 + 7] = var1;
            var26 += 8;
        }

        // Second pass: re-mix mem[] over itself.
        let mut var51 = 0usize;
        while var51 < 256 {
            let var52 = self.mem[var51].wrapping_add(var8);
            let var53 = self.mem[var51 + 1].wrapping_add(var7);
            let var54 = self.mem[var51 + 2].wrapping_add(var6);
            let var55 = self.mem[var51 + 3].wrapping_add(var5);
            let var56 = self.mem[var51 + 4].wrapping_add(var4);
            let var57 = self.mem[var51 + 5].wrapping_add(var3);
            let var58 = self.mem[var51 + 6].wrapping_add(var2);
            let var59 = self.mem[var51 + 7].wrapping_add(var1);
            let var60 = var52 ^ var53.wrapping_shl(11);
            let var61 = var55.wrapping_add(var60);
            let var62 = var53.wrapping_add(var54);
            let var63 = var62 ^ shr(var54, 2);
            let var64 = var56.wrapping_add(var63);
            let var65 = var54.wrapping_add(var61);
            let var66 = var65 ^ var61.wrapping_shl(8);
            let var67 = var57.wrapping_add(var66);
            let var68 = var61.wrapping_add(var64);
            var5 = var68 ^ shr(var64, 16);
            let var69 = var5.wrapping_add(var58);
            let var70 = var64.wrapping_add(var67);
            var4 = var70 ^ var67.wrapping_shl(10);
            let var71 = var4.wrapping_add(var59);
            let var72 = var67.wrapping_add(var69);
            var3 = var72 ^ shr(var69, 4);
            let var73 = var3.wrapping_add(var60);
            let var74 = var69.wrapping_add(var71);
            var2 = var74 ^ var71.wrapping_shl(8);
            var7 = var2.wrapping_add(var63);
            let var75 = var71.wrapping_add(var73);
            var1 = var75 ^ shr(var73, 9);
            var6 = var1.wrapping_add(var66);
            var8 = var7.wrapping_add(var73);
            self.mem[var51] = var8;
            self.mem[var51 + 1] = var7;
            self.mem[var51 + 2] = var6;
            self.mem[var51 + 3] = var5;
            self.mem[var51 + 4] = var4;
            self.mem[var51 + 5] = var3;
            self.mem[var51 + 6] = var2;
            self.mem[var51 + 7] = var1;
            var51 += 8;
        }

        self.generate();
        self.count = 256;
    }
}
