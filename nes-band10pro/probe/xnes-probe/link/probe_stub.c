/* Stage-1 link stub: references the Rust staticlib exports so the linker
 * pulls only reachable x-nes objects (no --whole-archive). */
typedef unsigned int u32;

extern int xnes_probe_boot(const unsigned char *data, u32 len);
extern const unsigned char *xnes_probe_frame(void);

const unsigned char *probe_run(const unsigned char *rom, u32 len)
{
    if (xnes_probe_boot(rom, len) != 0) {
        return 0;
    }
    return xnes_probe_frame();
}
