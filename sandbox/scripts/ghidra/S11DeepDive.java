// S11 D7/D8 deep-dive pass 1 (no-analysis mode): locate scheduler +
// segment-allocator candidates in IDMan.exe WITHOUT auto-analysis (its
// budget exceeds the sandbox; the ~47KB stub already needed 2.5 min).
// Method: raw C-string scan of initialized blocks for study needles and
// MSVC RTTI names; scanU32 back-reference walk for code references;
// RTTI chain (name <- TD+8 <- COL+12 <- vtable-4) to vtables; vtable
// slot enumeration for EXACT virtual-function entry points.
// Bounded: no decompilation at all here (pass 2 decompiles chosen starts).
// Output goes ONLY to a throwaway directory outside the repo (clean-room).
// Args: <outputDir>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import java.io.BufferedReader;
import java.io.File;
import java.io.FileReader;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public class S11DeepDive extends GhidraScript {

    // Functional needles distilled from the committed study (docs/RESEARCH-IDM-STUDY.md §4)
    static final String[] NEEDLES = {
        "Scheduler", "Queue", "QUEUEITEM", "SpeedLimiter", "SortSpeed",
        "LimitMonitor", "DownloadLimits", "optional connection", "Segment",
        // session-2 targets: worker-thread range arithmetic, queue-state writer,
        // site-login / refresh-link (D7:B commissioned scope, session 2)
        "one connection", "data corruption", "If-Range", "bytes=", "Range:",
        "Content-Range", "Accept-Ranges", "Authorization", "NTLM", "Negotiate",
        "Login", "Log in", "Password", "Cookie:", "Refresh", "Reconnect",
        "connection lost", "resume", "Resume", ".Part", ".part",
        "connection refused", "Keep-Alive", "keep-alive", "503", "502", "416"
    };
    // RTTI classes of interest (study RTTI distillation; contains-match)
    static final String[] RTTI_WANT = {
        "CDownloadQueue", "CQueuesArray", "CSpeedLimiter", "CDownloadLimits",
        "CLimitMonitor", "CDownloadSegment", "CSegment"
    };
    static final Pattern RTTI_NAME = Pattern.compile("^\\.\\?AV[A-Za-z0-9_@$]+@@");

    PrintWriter log;
    Map<String, TreeSet<String>> evidence = new TreeMap<String, TreeSet<String>>();

    PrintWriter open(String dir, String name) throws Exception {
        File f = new File(dir, name);
        f.getParentFile().mkdirs();
        return new PrintWriter(new FileWriter(f));
    }

    String trunc(String s, int n) {
        s = s.replace("\r", "\\r").replace("\n", "\\n").replace("\t", "\\t");
        return s.length() > n ? s.substring(0, n) + "..." : s;
    }

    void addEvidenceRaw(Address from, String ev) {
        TreeSet<String> set = evidence.get("RAW@" + from);
        if (set == null) {
            set = new TreeSet<String>();
            evidence.put("RAW@" + from, set);
        }
        set.add(ev);
    }

    // block cache: name -> [startOffset, bytes, executable]
    Map<String, Object[]> blocks = new LinkedHashMap<String, Object[]>();

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

    List<Address> scanU32(long value) {
        List<Address> out = new ArrayList<Address>();
        for (Object[] ob : blocks.values()) {
            Address start = (Address) ob[0];
            byte[] data = (byte[]) ob[1];
            int limit = data.length - 4;
            for (int i = 0; i <= limit; i++) {
                long v = (data[i] & 0xffL) | ((data[i + 1] & 0xffL) << 8)
                        | ((data[i + 2] & 0xffL) << 16) | ((data[i + 3] & 0xffL) << 24);
                if (v == value) {
                    out.add(start.add(i));
                }
            }
        }
        return out;
    }

    boolean inCode(Address a) {
        for (Object[] ob : blocks.values()) {
            Address start = (Address) ob[0];
            byte[] data = (byte[]) ob[1];
            if (((Boolean) ob[2])) {
                long off = a.getOffset() - start.getOffset();
                if (off >= 0 && off < data.length) {
                    return true;
                }
            }
        }
        return false;
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outDir = args.length > 0 ? args[0] : ".";
        new File(outDir).mkdirs();
        log = open(outDir, "deepdive-log.txt");
        log.println("program=" + currentProgram.getName()
                + " imageBase=" + currentProgram.getImageBase()
                + " mode=no-analysis bounded");
        println("S11DD| program=" + currentProgram.getName() + " (no-analysis mode)");

        loadBlocks();
        for (Map.Entry<String, Object[]> e : blocks.entrySet()) {
            log.println("block " + e.getKey() + " " + e.getValue()[0]
                    + " len=" + ((byte[]) e.getValue()[1]).length
                    + " exec=" + e.getValue()[2]);
        }

        // 1. raw C-string scan: printable ASCII runs (>=4 chars) with NUL
        Map<Long, String> strings = new TreeMap<Long, String>();
        for (Object[] ob : blocks.values()) {
            Address start = (Address) ob[0];
            byte[] data = (byte[]) ob[1];
            if (((Boolean) ob[2])) {
                continue; // code blocks hold few real strings; skip for speed
            }
            int run = 0;
            for (int i = 0; i < data.length; i++) {
                int c = data[i] & 0xff;
                if (c >= 32 && c < 127) {
                    run++;
                } else {
                    if (run >= 4) {
                        String s = new String(data, i - run, run);
                        strings.put(start.getOffset() + (i - run), s);
                    }
                    run = 0;
                }
            }
        }
        log.println("rawStrings=" + strings.size());
        println("S11DD| rawStrings=" + strings.size());

        // 2. match needles + RTTI names
        List<Object[]> targets = new ArrayList<Object[]>(); // [addrLong, value, isRTTI]
        for (Map.Entry<Long, String> e : strings.entrySet()) {
            String s = e.getValue();
            Matcher m = RTTI_NAME.matcher(s);
            if (m.matches()) {
                targets.add(new Object[] { e.getKey(), s, Boolean.TRUE });
            } else {
                String low = s.toLowerCase();
                for (String n : NEEDLES) {
                    if (low.contains(n.toLowerCase())) {
                        targets.add(new Object[] { e.getKey(), s, Boolean.FALSE });
                        break;
                    }
                }
            }
        }
        log.println("matchedTargets=" + targets.size());
        println("S11DD| matched=" + targets.size());
        try (PrintWriter w = open(outDir, "deepdive-strings-targets.txt")) {
            for (Object[] t : targets) {
                w.println(Long.toHexString((Long) t[0]) + "\t"
                        + ((Boolean) t[2] ? "RTTI" : "STR") + "\t"
                        + trunc((String) t[1], 120));
            }
        }

        // 3. plain-string targets: analyzer refs if any, else raw scan sites
        for (Object[] t : targets) {
            if ((Boolean) t[2]) {
                continue;
            }
            Address a = toAddr((Long) t[0]);
            String ev = "STR:" + trunc((String) t[1], 60);
            int n = 0;
            try {
                ReferenceIterator ri = currentProgram.getReferenceManager()
                        .getReferencesTo(a);
                while (ri.hasNext()) {
                    Reference r = ri.next();
                    addEvidenceRaw(r.getFromAddress(), ev);
                    n++;
                }
            } catch (Exception ignore) {
            }
            if (n == 0) {
                for (Address from : scanU32((Long) t[0])) {
                    if (inCode(from)) {
                        addEvidenceRaw(from, "STRUSE:" + trunc((String) t[1], 60));
                    }
                }
            }
        }

        // 4. RTTI chain walk (wanted classes only): name <- TD+8;
        //    TD <- COL+12; COL <- vtable-4; enumerate vtable slots -> fn starts
        Map<String, List<String>> fnStarts = new LinkedHashMap<String, List<String>>();
        int chains = 0;
        try (PrintWriter w = open(outDir, "deepdive-rtti-chains.txt")) {
            for (Object[] t : targets) {
                if (!(Boolean) t[2]) {
                    continue;
                }
                String cls = (String) t[1];
                boolean wanted = false;
                for (String want : RTTI_WANT) {
                    if (cls.contains(want)) {
                        wanted = true;
                    }
                }
                if (!wanted) {
                    continue;
                }
                long nameAddr = (Long) t[0];
                try {
                    List<Address> tdFields = scanU32(nameAddr); // TD+8 slots
                    for (Address td8 : tdFields) {
                        Address td = td8.subtract(8);
                        List<Address> colFields = scanU32(td.getOffset()); // COL+12
                        for (Address col12 : colFields) {
                            Address col = col12.subtract(12);
                            List<Address> vtMinus4s = scanU32(col.getOffset());
                            for (Address vm4 : vtMinus4s) {
                                Address vt = vm4.add(4);
                                // enumerate vtable slots while they point into code
                                List<String> slots = new ArrayList<String>();
                                for (int s = 0; s < 96; s++) {
                                    long slotAddr = vt.getOffset() + 4L * s;
                                    Object[] ob = blockAt(slotAddr);
                                    if (ob == null) {
                                        break;
                                    }
                                    byte[] d = (byte[]) ob[1];
                                    long off = slotAddr - ((Address) ob[0]).getOffset();
                                    if (off < 0 || off + 4 > d.length) {
                                        break;
                                    }
                                    long fn = (d[(int) off] & 0xffL)
                                            | ((d[(int) off + 1] & 0xffL) << 8)
                                            | ((d[(int) off + 2] & 0xffL) << 16)
                                            | ((d[(int) off + 3] & 0xffL) << 24);
                                    if (fn == 0 || !inCode(toAddr(fn))) {
                                        break;
                                    }
                                    slots.add(Long.toHexString(fn));
                                }
                                w.println(cls + "\tvt=" + vt + "\tslots=" + slots.size());
                                String key = cls + "@vt" + vt;
                                fnStarts.put(key, slots);
                                chains++;
                                for (Address u : scanU32(vt.getOffset())) {
                                    if (inCode(u)) {
                                        addEvidenceRaw(u, "VTASSIGN:" + cls);
                                    }
                                }
                            }
                        }
                    }
                } catch (Exception e) {
                    log.println("rtti chain failed for " + cls + ": " + e);
                }
            }
        }
        log.println("rtti vtables=" + chains);
        println("S11DD| rtti vtables=" + chains);

        // 5. dump function starts for pass 2
        try (PrintWriter w = open(outDir, "deepdive-fnstarts.txt")) {
            for (Map.Entry<String, List<String>> e : fnStarts.entrySet()) {
                w.println(e.getKey() + "\t" + String.join(",", e.getValue()));
            }
        }

        // 6. dump raw evidence sites (string-use points)
        try (PrintWriter w = open(outDir, "deepdive-rawsites.txt")) {
            for (Map.Entry<String, TreeSet<String>> e : evidence.entrySet()) {
                w.println(e.getKey() + "\t" + e.getValue());
            }
        }
        log.println("rawSites=" + evidence.size());
        println("S11DD| rawSites=" + evidence.size());
        log.flush();
        log.close();
        println("S11DD| done outDir=" + outDir);
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
}
