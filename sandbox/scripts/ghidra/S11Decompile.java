// S11 D7/D8 deep-dive pass 2: bounded targeted decompilation.
// Phase A: anchor sites (string-use points from pass 1) -> containing
//          functions via backward boundary search + disassembly validation.
// Phase B: corrected MSVC RTTI chains (TD = nameAddr-8, name is INLINE at
//          TD+8; COL+12 -> TD; vtable-4 -> COL) -> vtable slots = exact
//          virtual-function entry points.
// Phase C: one level of callee crawl (bounded).
// Budget-bounded: hard internal time budget, per-function decompile
// timeout, caps on slots/callees; honest counts logged.
// Output goes ONLY to a throwaway directory outside the repo (clean-room).
// Args: <outputDir> [budgetSeconds]
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.mem.MemoryBlock;
import java.io.File;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public class S11Decompile extends GhidraScript {

    // anchor sites (Ghidra VAs) from pass-1 evidence — scheduler + segment machinery
    static final Object[][] ANCHORS = {
        { 0x49c74eL, "reg_Scheduler" }, { 0x49c8c1L, "reg_Scheduler2" },
        { 0x49dbc0L, "sched_s_dt" }, { 0x49dd07L, "sched_s_dt2" },
        { 0x49deaeL, "sched_s_dt3" }, { 0x49defdL, "queue_q_dt" },
        { 0x49e258L, "queue_q_dt2" }, { 0x49ec5dL, "queue_q_dt3" },
        { 0x49e779L, "cdq_ctor_reg" }, { 0x49e802L, "cdq_ctor_new" },
        { 0x49e89bL, "cdq_nextitem" }, { 0x4b7db0L, "SortSpeed" },
        { 0x465fc7L, "CheckQueueState2" }, { 0x4ecd2cL, "optconn_created" },
        { 0x4ed173L, "optconn_connecting" }, { 0x4ed488L, "queue_finished" },
        { 0x4ed5b4L, "queue_started" }, { 0x57d17cL, "need_optconn" },
        { 0x4ef7e3L, "corrupt_one_conn" }, { 0x49d67eL, "reg_Queue" },
    };
    // RTTI classes whose vtables we walk (names inline at TD+8)
    static final String[] RTTI_CLASSES = {
        "CDownloadQueue", "CDownload", "CQueuesArray", "CSchedulerInfo",
        "CLimitMonitor", "CDownloadLimits", "CDownloaderDlg",
        "CDownloaderApp",
    };

    PrintWriter log;
    Map<String, Object[]> blocks = new LinkedHashMap<String, Object[]>();
    DecompInterface di;
    long t0;
    long budgetMs = 380_000L;
    PrintWriter outC;
    Map<Long, Function> decompiled = new LinkedHashMap<Long, Function>();
    Set<String> calleeQueue = new java.util.LinkedHashSet<String>();

    PrintWriter open(String dir, String name) throws Exception {
        File f = new File(dir, name);
        f.getParentFile().mkdirs();
        return new PrintWriter(new FileWriter(f));
    }

    boolean overBudget() {
        return System.currentTimeMillis() - t0 > budgetMs;
    }

    void loadBlocks() throws Exception {
        for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
            if (!b.isInitialized() || b.getSize() > 64 * 1024 * 1024) {
                continue;
            }
            byte[] data = new byte[(int) b.getSize()];
            try {
                currentProgram.getMemory().getBytes(b.getStart(), data);
            } catch (Exception e) {
                continue;
            }
            boolean exec = false;
            try {
                exec = b.isExecute();
            } catch (Exception ignore) {
                exec = b.getName().toLowerCase().contains("text");
            }
            blocks.put(b.getName(), new Object[] { b.getStart(), data, exec });
        }
    }

    Object[] blockAt(long addr) {
        for (Object[] ob : blocks.values()) {
            Address start = (Address) ob[0];
            byte[] data = (byte[]) ob[1];
            long off = addr - start.getOffset();
            if (off >= 0 && off < data.length) {
                return ob;
            }
        }
        return null;
    }

    boolean inCode(long addr) {
        Object[] ob = blockAt(addr);
        return ob != null && ((Boolean) ob[2]);
    }

    int u32At(long addr) {
        Object[] ob = blockAt(addr);
        if (ob == null) {
            return -1;
        }
        byte[] d = (byte[]) ob[1];
        long off = addr - ((Address) ob[0]).getOffset();
        if (off < 0 || off + 4 > d.length) {
            return -1;
        }
        return (int) ((d[(int) off] & 0xffL) | ((d[(int) off + 1] & 0xffL) << 8)
                | ((d[(int) off + 2] & 0xffL) << 16) | ((d[(int) off + 3] & 0xffL) << 24));
    }

    List<Long> scanU32(long value) {
        List<Long> out = new ArrayList<Long>();
        for (Object[] ob : blocks.values()) {
            Address start = (Address) ob[0];
            byte[] data = (byte[]) ob[1];
            int limit = data.length - 4;
            for (int i = 0; i <= limit; i++) {
                long v = (data[i] & 0xffL) | ((data[i + 1] & 0xffL) << 8)
                        | ((data[i + 2] & 0xffL) << 16) | ((data[i + 3] & 0xffL) << 24);
                if (v == value) {
                    out.add(start.getOffset() + i);
                }
            }
        }
        return out;
    }

    byte bAt(long addr) {
        Object[] ob = blockAt(addr);
        if (ob == null) {
            return 0;
        }
        byte[] d = (byte[]) ob[1];
        long off = addr - ((Address) ob[0]).getOffset();
        if (off < 0 || off >= d.length) {
            return 0;
        }
        return d[(int) off];
    }

    // backward search for the function start containing `site` (the site is
    // the imm32 operand address — e.g. push imm32 opcode sits at site-1)
    long findStart(long site) {
        // exact-start: push-imm32 opcode at site-1 right after a ret/pad
        if (bAt(site - 1) == 0x68 && afterRetOrPad(site - 1)) {
            return site - 1;
        }
        long textStart = ((Address) blocks.get(".text")[0]).getOffset();
        for (long p = site - 1; p >= site - 8192 && p >= textStart; p--) {
            if (plausiblePrologue(p) && afterRetOrPad(p)) {
                // validate: disassemble from p; site must be INSIDE an instruction
                try {
                    disassemble(toAddr(p));
                    Instruction ins = currentProgram.getListing()
                            .getInstructionContaining(toAddr(site));
                    if (ins != null) {
                        return p;
                    }
                } catch (Exception ignore) {
                }
            }
        }
        return -1;
    }

    boolean plausiblePrologue(long p) {
        byte b = bAt(p);
        // push imm32 (trace idiom), push ebp; mov ebp,esp, mov edi/mov reg reg,
        // push esi/ebx/edi/ecx/edx, sub esp, jmp
        if (b == 0x68) {
            return true;
        }
        if (b == 0x55 && bAt(p + 1) == (byte) 0x8b && bAt(p + 2) == (byte) 0xec) {
            return true;
        }
        if (b == 0x8b && bAt(p + 1) == (byte) 0xff) {
            return true;
        }
        if (b >= 0x50 && b <= 0x57) {
            return true;
        }
        if (b == 0x83 && bAt(p + 1) == (byte) 0xec) {
            return true;
        }
        if (b == 0xe9) {
            return true;
        }
        if (b == 0x81 && bAt(p + 1) == (byte) 0xec) {
            return true;
        }
        return false;
    }

    boolean afterRetOrPad(long p) {
        // boundary before p: ret / ret n / int3 / nop
        byte b1 = bAt(p - 1);
        if (b1 == 0xc3 || b1 == (byte) 0xcc || b1 == 0x90) {
            return true;
        }
        if (bAt(p - 3) == (byte) 0xc2) { // ret imm16 occupies p-3..p-1
            return true;
        }
        return false;
    }

    Function decompileAt(long start, String tag) throws Exception {
        if (decompiled.containsKey(start)) {
            return decompiled.get(start);
        }
        if (overBudget()) {
            log.println("BUDGET-STOP before " + tag);
            return null;
        }
        Function f = currentProgram.getFunctionManager().getFunctionAt(toAddr(start));
        if (f == null) {
            disassemble(toAddr(start));
            f = createFunction(toAddr(start), "S11_" + tag);
        }
        if (f == null) {
            log.println("createFunction failed @" + Long.toHexString(start) + " " + tag);
            return null;
        }
        DecompileResults r = di.decompileFunction(f, 30, monitor);
        if (r != null && r.decompileCompleted() && r.getDecompiledFunction() != null) {
            String c = r.getDecompiledFunction().getC();
            outC.println("/* ===== S11_" + tag + " @ " + Long.toHexString(start)
                    + " body=" + f.getBody().getNumAddresses() + " ===== */");
            outC.println(c);
            outC.flush();
            decompiled.put(start, f);
            log.println("decompiled " + tag + " @" + Long.toHexString(start)
                    + " body=" + f.getBody().getNumAddresses());
            // queue callees
            Matcher m = Pattern.compile("FUN_([0-9a-fA-F]{8})").matcher(c);
            while (m.find()) {
                calleeQueue.add(m.group(1));
            }
            return f;
        }
        log.println("decompile FAILED " + tag + " @" + Long.toHexString(start));
        return null;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outDir = args.length > 0 ? args[0] : ".";
        if (args.length > 1) {
            budgetMs = Long.parseLong(args[1]) * 1000L;
        }
        new File(outDir).mkdirs();
        t0 = System.currentTimeMillis();
        log = open(outDir, "decompile-log.txt");
        outC = open(outDir, "decomp-targets.c");
        println("S11DC| program=" + currentProgram.getName());
        loadBlocks();

        di = new DecompInterface();
        di.openProgram(currentProgram);

        // ---- Phase A: anchor functions ----
        Map<Long, String> anchorFns = new TreeMap<Long, String>();
        for (Object[] a : ANCHORS) {
            if (overBudget()) {
                break;
            }
            long site = (Long) a[0];
            String tag = (String) a[1];
            long start = findStart(site);
            if (start < 0) {
                log.println("anchor " + tag + " site=" + Long.toHexString(site)
                        + ": NO START FOUND");
                println("S11DC| anchor " + tag + " no-start");
                continue;
            }
            if (!anchorFns.containsKey(start)) {
                anchorFns.put(start, tag);
            } else {
                anchorFns.put(start, anchorFns.get(start) + "+" + tag);
            }
            log.println("anchor " + tag + " site=" + Long.toHexString(site)
                    + " -> start " + Long.toHexString(start));
        }
        for (Map.Entry<Long, String> e : anchorFns.entrySet()) {
            if (overBudget()) {
                break;
            }
            decompileAt(e.getKey(), e.getValue());
        }
        println("S11DC| anchorFns=" + anchorFns.size());

        // ---- Phase B: RTTI vtables (TD = nameAddr-8; COL+12 -> TD; vt-4 -> COL) ----
        Map<Long, String> rttiFns = new TreeMap<Long, String>();
        int vts = 0;
        for (String cls : RTTI_CLASSES) {
            if (overBudget()) {
                break;
            }
            // find the inline RTTI name string: ".?AV<Class>@@"
            boolean foundName = false;
            for (Object[] ob : blocks.values()) {
                if (((Boolean) ob[2])) {
                    continue;
                }
                Address bstart = (Address) ob[0];
                byte[] data = (byte[]) ob[1];
                String want = ".?AV" + cls + "@@";
                byte[] wb = want.getBytes("US-ASCII");
                int limit = data.length - wb.length;
                for (int i = 0; i <= limit; i++) {
                    if (data[i] == '.' && data[i + 1] == '?') {
                        boolean match = true;
                        for (int k = 0; k < wb.length; k++) {
                            if (data[i + k] != wb[k]) {
                                match = false;
                                break;
                            }
                        }
                        if (match) {
                            long nameAddr = bstart.getOffset() + i;
                            long td = nameAddr - 8;
                            for (Long col12 : scanU32(td)) {
                                long col = col12 - 12;
                                for (Long vm4 : scanU32(col)) {
                                    long vt = vm4 + 4;
                                    // enumerate slots
                                    int slots = 0;
                                    for (int s = 0; s < 96; s++) {
                                        long fn = vt + 4L * s;
                                        int val = u32At(fn);
                                        if (val <= 0 || !inCode(val & 0xffffffffL)) {
                                            break;
                                        }
                                        slots++;
                                        rttiFns.put(val & 0xffffffffL,
                                                cls + "_vt" + s);
                                    }
                                    log.println("rtti " + cls + " td=" + Long.toHexString(td)
                                            + " vt=" + Long.toHexString(vt)
                                            + " slots=" + slots);
                                    vts++;
                                }
                            }
                            foundName = true;
                            i = limit; // stop scanning this block
                        }
                    }
                }
                if (foundName) {
                    break;
                }
            }
            if (!foundName) {
                log.println("rtti " + cls + ": name not found");
            }
        }
        log.println("rtti vtables=" + vts + " slotFns=" + rttiFns.size());
        println("S11DC| rtti vts=" + vts + " slotFns=" + rttiFns.size());

        // decompile vtable slots (cap per class applied during collection: 96)
        int slotDone = 0;
        for (Map.Entry<Long, String> e : rttiFns.entrySet()) {
            if (overBudget()) {
                break;
            }
            if (decompileAt(e.getKey(), e.getValue()) != null) {
                slotDone++;
            }
        }
        println("S11DC| rttiFns decompiled=" + slotDone);

        // ---- Phase C: callee crawl (bounded; iterate over a snapshot) ----
        int callees = 0;
        for (String hex : new ArrayList<String>(calleeQueue)) {
            if (overBudget() || callees >= 48) {
                break;
            }
            long addr = Long.parseLong(hex, 16);
            if (!inCode(addr) || decompiled.containsKey(addr)) {
                continue;
            }
            if (decompileAt(addr, "callee_" + hex) != null) {
                callees++;
            }
        }
        println("S11DC| callees=" + callees);

        log.println("totalDecompiled=" + decompiled.size()
                + " elapsedMs=" + (System.currentTimeMillis() - t0));
        println("S11DC| total=" + decompiled.size());
        outC.flush();
        log.flush();
        log.close();
        di.dispose();
        println("S11DC| done outDir=" + outDir);
    }
}
