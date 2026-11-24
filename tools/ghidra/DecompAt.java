// Ghidra headless post-script: decompile the function at each address; create it first when the analysis missed it
// (entries reached only through data tables, e.g. the action handlers at 0x703880 +0xc). Changes are not saved (-readOnly).
// usage: analyzeHeadless DIR PROJ -process target.bin -noanalysis -readOnly -scriptPath tools/ghidra -postScript DecompAt.java OUT ADDR...
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;
import java.io.*;

public class DecompAt extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter c = new PrintWriter(new FileWriter(a[0]));
        DecompInterface di = new DecompInterface();
        di.openProgram(currentProgram);
        for (int i = 1; i < a.length; i++) {
            Address at = toAddr(a[i]);
            Function f = getFunctionAt(at);
            boolean made = f == null;
            if (made) { disassemble(at); f = createFunction(at, null); }
            c.println("// " + at + (f == null ? " (no function)" : " " + f.getName() + (made ? " (created here)" : "")));
            if (f == null) continue;
            DecompileResults r = di.decompileFunction(f, 60, monitor);
            c.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// decompile failed: " + r.getErrorMessage());
            c.flush();
        }
        c.close(); di.dispose();
        println("DecompAt: DONE " + (a.length - 1));
    }
}
