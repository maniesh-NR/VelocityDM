// S11 deep-dive helper (session 2): dump every printable ASCII run (>=4 chars)
// found in initialized blocks as addr<TAB>text TSV. No analysis required; pairs
// with s11_annotate.py to make no-analysis decompiled sections readable.
// Output goes ONLY to the throwaway directory (clean-room). Args: <outDir>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.mem.MemoryBlock;
import java.io.File;
import java.io.FileWriter;
import java.io.PrintWriter;

public class S11DumpStrings extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File out = new File(args.length > 0 ? args[0] : ".");
        out.mkdirs();
        PrintWriter w = new PrintWriter(new FileWriter(new File(out, "strings.tsv")));
        long count = 0;
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
            int run = 0;
            for (int i = 0; i <= data.length; i++) {
                int c = i < data.length ? (data[i] & 0xff) : 0;
                if (c >= 32 && c < 127) {
                    run++;
                    continue;
                }
                if (run >= 4) {
                    String s = new String(data, i - run, run);
                    long addr = b.getStart().getOffset() + (i - run);
                    // one line each; keep tabs out of the text column
                    w.println(Long.toHexString(addr) + "\t"
                            + s.replace("\t", " ").replace("\n", " ").replace("\r", " "));
                    count++;
                }
                run = 0;
            }
        }
        w.flush();
        w.close();
        println("S11DS| strings=" + count + " out=" + out);
    }
}
