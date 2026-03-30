# GBA debug configuration for gdb-multiarch
# Usage: make debug (terminal 1), make gdb (terminal 2)

# Connect to mGBA GDB stub
target remote localhost:2345

# GBA memory dump helpers
define iwram
  x/64xw 0x03000000
end
document iwram
  Dump first 256 bytes of IWRAM (data + BSS region)
end

define ewram
  x/64xw 0x02000000
end
document ewram
  Dump first 256 bytes of EWRAM (game state region)
end

define stack
  x/16xw $sp
end
document stack
  Dump 64 bytes from current stack pointer
end

define canary
  x/4xw 0x03006000
end
document canary
  Inspect the 4 stack canary words (expect 0xDEADBEEF if intact)
end

define regs
  info registers r0 r1 r2 r3 r4 r5 r6 r7 r8 r9 r10 r11 r12 sp lr pc cpsr
end
document regs
  Show all ARM registers
end

# Auto-break on panic
break panic_handler

# Start execution
continue
