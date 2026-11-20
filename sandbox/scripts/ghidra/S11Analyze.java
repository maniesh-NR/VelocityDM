// S11 T2 static-RE helper: inventory + full decompilation dump of a small PE.
// Output goes ONLY to a throwaway directory outside the repo (clean-room rule).
// Args: <outputDir>
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.ExternalLocation;
import ghidra.program.model.symbol.ExternalManager;
import java.io.File;
import java.io.FileWriter;
import java.io.PrintWriter;

public class S11Analyze extends GhidraScript {

        private PrintWriter open(String dir, String name) throws Exception {
                File f = new File(dir, name);
                f.getParentFile().mkdirs();
                return new PrintWriter(new FileWriter(f));
        }

        @Override
        public void run() throws Exception {
                String[] args = getScriptArgs();
                String outDir = args.length > 0 ? args[0] : ".";
                new File(outDir).mkdirs();
                println("S11| program=" + currentProgram.getName() +
                        " lang=" + currentProgram.getLanguageID() +
                        " imageBase=" + currentProgram.getImageBase());

                // 1. memory blocks (shows mapped layout incl. overlay/resources)
                try (PrintWriter w = open(outDir, "blocks.txt")) {
                        for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                                w.println(b.getName() + "\t" + b.getStart() + "\t" + b.getSize()
                                        + "\t" + (b.isInitialized() ? "init" : "uninit"));
                        }
                } catch (Exception e) {
                        println("S11| blocks failed: " + e);
                }

                // 2. defined strings
                try (PrintWriter w = open(outDir, "strings.txt")) {
                        DataIterator it = currentProgram.getListing().getDefinedData(true);
                        while (it.hasNext()) {
                                Data d = it.next();
                                try {
                                        if (d.hasStringValue()) {
                                                Object v = d.getValue();
                                                String s = v == null ? "" : v.toString();
                                                if (s.length() >= 4) {
                                                        w.println(d.getAddress() + "\t" +
                                                                s.replace("\r", "\\r").replace("\n", "\\n"));
                                                }
                                        }
                                } catch (Exception ignore) {
                                        // skip malformed entries
                                }
                        }
                } catch (Exception e) {
                        println("S11| strings failed: " + e);
                }

                // 4. function inventory
                int fnCount = 0;
                try (PrintWriter w = open(outDir, "functions.txt")) {
                        FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
                        while (fit.hasNext()) {
                                Function f = fit.next();
                                w.println(f.getEntryPoint() + "\t" + f.getName() + "\t" +
                                        f.getBody().getNumAddresses());
                                fnCount++;
                        }
                } catch (Exception e) {
                        println("S11| functions failed: " + e);
                }
                println("S11| functions=" + fnCount);

                // 5. decompile every function (bounded body size), single dump file
                DecompInterface di = new DecompInterface();
                try {
                        di.openProgram(currentProgram);
                        int decompiled = 0;
                        try (PrintWriter w = open(outDir, "decomp_all.c")) {
                                FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
                                while (fit.hasNext()) {
                                        if (monitor.isCancelled()) {
                                                break;
                                        }
                                        Function f = fit.next();
                                        if (f.getBody().getNumAddresses() > 20000) {
                                                continue; // skip pathological thunks/data blobs
                                        }
                                        DecompileResults r = di.decompileFunction(f, 120, monitor);
                                        if (r != null && r.decompileCompleted() && r.getDecompiledFunction() != null) {
                                                w.println("/* ===== " + f.getName() + " @ " + f.getEntryPoint() +
                                                        " ===== */");
                                                w.println(r.getDecompiledFunction().getC());
                                                decompiled++;
                                        }
                                }
                        }
                        println("S11| decompiled=" + decompiled);
                } finally {
                        di.dispose();
                }
                println("S11| done outDir=" + outDir);
        }
}
