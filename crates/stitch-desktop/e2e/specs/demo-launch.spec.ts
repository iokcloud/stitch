/**
 * Launch demo: real-model desktop automation task with full-screen frame
 * capture (PowerShell GDI). Frames → e2e/artifacts/demo-launch/frames/
 * Post-process: python scripts/demo-gif.py (pillow → demo.gif for README).
 */
import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  bootChat,
  clickSend,
  fillChat,
  newSession,
  setWorkDir,
  waitIdle,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/demo-launch");
const framesDir = path.join(outDir, "frames");

let frameSeq = 0;
function captureFrame(label: string): void {
  frameSeq += 1;
  const p = path.join(framesDir, `frame-${String(frameSeq).padStart(3, "0")}-${label}.png`);
  const ps = p.replace(/'/g, "''");
  try {
    execSync(
      `powershell -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; ` +
        `$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; ` +
        `$bmp=New-Object System.Drawing.Bitmap($b.Width,$b.Height); ` +
        `$g=[System.Drawing.Graphics]::FromImage($bmp); ` +
        `$g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); ` +
        `$bmp.Save('${ps}'); $g.Dispose(); $bmp.Dispose();"`,
      { timeout: 15000 },
    );
    console.log(`frame: ${p}`);
  } catch {
    // capture is best-effort; never fail the demo on a lost frame
  }
}

describe("Launch demo (real model)", () => {
  before(async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(framesDir, { recursive: true });
    await bootChat();
  });

  it("desktop automation task with frame capture", async () => {
    await newSession();
    await setWorkDir(path.join(__dirname, "../../../test_workspace"));
    captureFrame("welcome");

    const task =
      "帮我用记事本创建一个文本文件，内容写『Stitch 演示：这是一段由 AI 智能体在本机自动写入的文字。』，保存到桌面，文件名叫 stitch-demo.txt";
    await fillChat(task);
    captureFrame("composed");
    await clickSend();
    captureFrame("sent");

    // Poll every ~2.5s while the agent works; stop when idle or after 90s.
    const deadline = Date.now() + 90_000;
    let idle = false;
    while (Date.now() < deadline && !idle) {
      await new Promise((r) => setTimeout(r, 2500));
      captureFrame("working");
      try {
        idle = await waitIdle(500);
      } catch {
        idle = false;
      }
    }
    captureFrame("done");
  });
});
