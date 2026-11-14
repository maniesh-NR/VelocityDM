// ProbeS11 — S11 T2 toolchain feasibility probe for Ghidra headless.
//
// Verifies, in one analyzeHeadless run: script compilation at runtime
// (requires a real JDK), program import + auto-analysis, function
// discovery, and headless C decompilation of a targeted routine via
// DecompInterface — the exact mechanism S11-T2 would use to study
// targeted routines of a pinned binary.
//
// Output lines are prefixed "PROBE"/"C|" for log grepping.
//
// Apache-2.0 Ghidra script; written for VelocityDM (GPL-3.0) — this
// file is original VelocityDM code and carries the project license.

import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;

public class ProbeS11 extends GhidraScript {
    @Override
    public void run() throws Exception {
        FunctionManager fm = currentProgram.getFunctionManager();
        println("PROBE program=" + currentProgram.getName()
            + " processor=" + currentProgram.getLanguage().getLanguageDescription().getProcessor()
            + " size=" + currentProgram.getLanguage().getLanguageDescription().getSize()
            + " endian=" + currentProgram.getLanguage().getLanguageDescription().getEndian());
        println("PROBE functions=" + fm.getFunctionCount());

        DecompInterface di = new DecompInterface();
        try {
            di.openProgram(currentProgram);
            Function target = null;
            for (Function f : fm.getFunctions(true)) {
                if (f.isThunk()) {
                    continue;
                }
                if (target == null) {
                    target = f; // first non-thunk as fallback
                }
                if ("main".equals(f.getName())) {
                    target = f;
                    break;
                }
            }
            if (target == null) {
                println("PROBE no-target-function");
                return;
            }
            DecompileResults r = di.decompileFunction(target, 60, monitor);
            if (r != null && r.decompileCompleted() && r.getDecompiledFunction() != null) {
                String c = r.getDecompiledFunction().getC();
                String[] lines = c.split("\n", -1);
                println("PROBE decompiled=" + target.getName() + " lines=" + lines.length);
                int n = Math.min(lines.length, 45);
                for (int i = 0; i < n; i++) {
                    println("C|" + lines[i]);
                }
            } else {
                println("PROBE decompile-failed="
                    + (r == null ? "null-results" : r.getErrorMessage()));
            }
        } finally {
            di.dispose();
        }
    }
}
