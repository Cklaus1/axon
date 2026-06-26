/*
 * R21 — Zephyr application entry that hands off to Axon.
 *
 * This is the C `main()` of a normal Zephyr app. It does two things:
 *   1. Provides `axon_console_putc(int)` — the extern C hook the Axon program
 *      calls (Axon's `zephyr_console_putc` builtin lowers to a call to it). We
 *      wire it to `printk`, so every byte the Axon code emits appears on the
 *      Zephyr console (the QEMU UART under `west build -t run`).
 *   2. Calls `axon_main()` — the Axon `@[entry]`-annotated function, compiled
 *      into the freestanding object linked alongside this file.
 *
 * The Axon object carries NO libc / axon-rt dependency; the single undefined
 * symbol it references is `axon_console_putc`, defined right here.
 */

#include <stdint.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

/* The Axon @[entry] symbol (examples/zephyr/app.ax → fn axon_main). */
extern void axon_main(void);

/*
 * The console hook the Axon code calls for output. `b` is a single byte
 * (0..255) passed as a C `int`. printk("%c", ...) emits it to the active
 * Zephyr console backend (the QEMU UART for qemu_cortex_m3).
 */
void axon_console_putc(int b)
{
	printk("%c", (char)b);
}

/*
 * Axon runtime trap hooks. A *freestanding* Axon image carries no axon-rt, so
 * the checked-arithmetic / refinement / bounds traps that codegen emits as
 * guarded calls reference these symbols. On a Zephyr target the host RTOS owns
 * panic semantics (the R17 "panic routes to a TCB component" model): we report
 * and hand off to Zephyr's panic. These match the axon-rt ABI signatures
 * (crates/axon-rt/src/lib.rs) and are marked noreturn.
 *
 * Marked __weak so an app that *does* link axon-rt (or a richer handler) can
 * override them.
 */
__weak FUNC_NORETURN void __axon_arith_panic(int64_t kind, const char *op_ptr,
					     int64_t op_len, int64_t a, int64_t b)
{
	ARG_UNUSED(op_ptr);
	ARG_UNUSED(op_len);
	printk("AXON: arithmetic panic (kind=%lld, a=%lld, b=%lld)\n",
	       (long long)kind, (long long)a, (long long)b);
	k_panic();
	CODE_UNREACHABLE;
}

__weak FUNC_NORETURN void __axon_bounds_panic(int64_t idx, int64_t len)
{
	printk("AXON: index %lld out of bounds (len %lld)\n", (long long)idx,
	       (long long)len);
	k_panic();
	CODE_UNREACHABLE;
}

__weak FUNC_NORETURN void __axon_refine_panic(const char *fn_ptr, int64_t fn_len,
					      const char *param_ptr,
					      int64_t param_len,
					      const char *refine_ptr,
					      int64_t refine_len)
{
	ARG_UNUSED(fn_ptr);
	ARG_UNUSED(fn_len);
	ARG_UNUSED(param_ptr);
	ARG_UNUSED(param_len);
	ARG_UNUSED(refine_ptr);
	ARG_UNUSED(refine_len);
	printk("AXON: refinement violated\n");
	k_panic();
	CODE_UNREACHABLE;
}

int main(void)
{
	printk("Zephyr main(): handing off to Axon\n");
	axon_main();
	printk("Zephyr main(): Axon returned\n");
	return 0;
}
