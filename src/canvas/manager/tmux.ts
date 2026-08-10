export interface TmuxSpawnOptions {
  command: string;
  title: string;
  sessionID: string;
  activate?: boolean;
  splitPercent?: number;
}

export interface TmuxSpawnResult {
  paneID: string;
  visible: boolean;
}

export type CanvasLayoutType =
  'single' | 'columns' | 'rows' | 'main-left' | 'main-right' | 'main-top' | 'main-bottom' | 'grid';

export interface TmuxLayoutOptions {
  sessionID: string;
  layout: CanvasLayoutType;
  paneIDs: string[];
  allPaneIDs: string[];
  focusPaneID?: string;
  mainPercent?: number;
}

export interface TmuxLayoutResult {
  layout: CanvasLayoutType;
  visiblePaneIDs: string[];
  focusedPaneID?: string;
}

interface SessionLayoutState {
  focusedPaneID?: string;
  visiblePaneIDs: Set<string>;
  layout: CanvasLayoutType;
  mainPercent: number;
}

interface TmuxRunResult {
  stdout: string;
  stderr: string;
}

async function streamText(stream: ReadableStream<Uint8Array> | null): Promise<string> {
  if (!stream) return '';
  return new Response(stream).text();
}

export function shellQuote(value: string): string {
  return "'" + value.replace(/'/g, "'\\''") + "'";
}

export class TmuxManager {
  private hostPaneID?: string;
  private tmuxSessionID?: string;
  private readonly sessionLayouts = new Map<string, SessionLayoutState>();

  activePaneID(sessionID: string): string | undefined {
    return this.layoutState(sessionID).focusedPaneID;
  }

  visiblePaneIDs(sessionID: string): string[] {
    return [...this.layoutState(sessionID).visiblePaneIDs];
  }

  currentLayout(sessionID: string): CanvasLayoutType {
    return this.layoutState(sessionID).layout;
  }

  forgetSession(sessionID: string): void {
    this.sessionLayouts.delete(sessionID);
  }

  async spawn(options: TmuxSpawnOptions): Promise<TmuxSpawnResult> {
    this.ensureTmux();
    await this.deactivateOtherSessions(options.sessionID);
    const state = this.layoutState(options.sessionID);

    const activePaneID = state.focusedPaneID ?? [...state.visiblePaneIDs][0];
    const activeStillExists = activePaneID ? await this.paneExists(activePaneID) : false;

    if (!activeStillExists) {
      const paneID = await this.splitVisible(options.command, options.splitPercent ?? 67);
      if (options.activate !== false) {
        await this.run(['select-pane', '-t', paneID]);
        state.focusedPaneID = paneID;
      } else {
        state.focusedPaneID = undefined;
      }
      state.visiblePaneIDs = new Set([paneID]);
      state.layout = 'single';
      state.mainPercent = 60;
      return { paneID, visible: true };
    }

    const paneID = await this.spawnHidden(options.command, options.title);
    if (options.activate === false) {
      return { paneID, visible: false };
    }

    await this.switchTo(options.sessionID, paneID);
    return { paneID, visible: true };
  }

  async switchTo(sessionID: string, paneID: string): Promise<void> {
    this.ensureTmux();
    await this.deactivateOtherSessions(sessionID);
    const state = this.layoutState(sessionID);

    if (!(await this.paneExists(paneID))) {
      throw new Error(`tmux pane does not exist: ${paneID}`);
    }

    if (state.visiblePaneIDs.has(paneID)) {
      await this.run(['select-pane', '-t', paneID]);
      state.focusedPaneID = paneID;
      return;
    }

    const replacedPaneID = state.focusedPaneID ?? [...state.visiblePaneIDs][0];
    const activeStillExists = replacedPaneID ? await this.paneExists(replacedPaneID) : false;

    if (!activeStillExists) {
      const hostPaneID = await this.getHostPaneID();
      await this.run(['join-pane', '-h', '-l', '67%', '-s', paneID, '-t', hostPaneID]);
      await this.run(['select-pane', '-t', paneID]);
      state.focusedPaneID = paneID;
      state.visiblePaneIDs.add(paneID);
      return;
    }

    await this.run(['swap-pane', '-s', paneID, '-t', replacedPaneID!]);
    await this.run(['select-pane', '-t', paneID]);
    state.visiblePaneIDs.delete(replacedPaneID!);
    state.visiblePaneIDs.add(paneID);
    state.focusedPaneID = paneID;
  }

  async next(sessionID: string, paneIDs: string[]): Promise<string | undefined> {
    await this.deactivateOtherSessions(sessionID);
    const state = this.layoutState(sessionID);
    const available: string[] = [];
    for (const paneID of paneIDs) {
      if (await this.paneExists(paneID)) {
        available.push(paneID);
      }
    }

    if (!available.length) return undefined;
    const visible = available.filter((paneID) => state.visiblePaneIDs.has(paneID));
    if (!visible.length) {
      await this.switchTo(sessionID, available[0]);
      return available[0];
    }

    if (visible.length === 1 && available.length > 1) {
      const currentIndex = available.indexOf(visible[0]);
      const nextPaneID = available[(currentIndex + 1) % available.length];
      await this.switchTo(sessionID, nextPaneID);
      return nextPaneID;
    }

    const currentIndex = visible.indexOf(state.focusedPaneID ?? '');
    const nextIndex = currentIndex === -1 ? 0 : (currentIndex + 1) % visible.length;
    const nextPaneID = visible[nextIndex];
    await this.switchTo(sessionID, nextPaneID);
    return nextPaneID;
  }

  async closePane(sessionID: string, paneID: string, fallbackPaneID?: string): Promise<void> {
    const state = this.layoutState(sessionID);
    const wasVisible = state.visiblePaneIDs.delete(paneID);
    if (state.focusedPaneID === paneID) {
      if (fallbackPaneID && fallbackPaneID !== paneID && (await this.paneExists(fallbackPaneID))) {
        state.focusedPaneID = undefined;
        if (state.visiblePaneIDs.has(fallbackPaneID)) {
          await this.run(['select-pane', '-t', fallbackPaneID]);
          state.focusedPaneID = fallbackPaneID;
        } else {
          await this.switchTo(sessionID, fallbackPaneID);
        }
      } else {
        const nextVisible = [...state.visiblePaneIDs][0];
        state.focusedPaneID = nextVisible;
        if (nextVisible) await this.run(['select-pane', '-t', nextVisible]);
      }
    }

    if (await this.paneExists(paneID)) {
      await this.run(['kill-pane', '-t', paneID]);
    }
    if (wasVisible && state.visiblePaneIDs.size <= 1) {
      state.layout = 'single';
      state.mainPercent = 60;
    }
  }

  async applyLayout(options: TmuxLayoutOptions): Promise<TmuxLayoutResult> {
    this.ensureTmux();
    await this.deactivateOtherSessions(options.sessionID);
    const state = this.layoutState(options.sessionID);
    const knownLayouts: CanvasLayoutType[] = [
      'single',
      'columns',
      'rows',
      'main-left',
      'main-right',
      'main-top',
      'main-bottom',
      'grid',
    ];
    if (!knownLayouts.includes(options.layout)) {
      throw new Error(`Unknown Canvas layout: ${options.layout}`);
    }
    const paneIDs = [...new Set(options.paneIDs)];
    if (!paneIDs.length) throw new Error('Canvas layout requires at least one pane');
    if (paneIDs.length > 4) throw new Error('Canvas layout supports at most four visible panes');
    if (options.layout === 'single' && paneIDs.length !== 1) {
      throw new Error('single layout requires exactly one Canvas');
    }
    if (options.layout !== 'single' && paneIDs.length < 2) {
      throw new Error(`${options.layout} layout requires at least two Canvases`);
    }
    if (options.layout === 'grid' && paneIDs.length < 2) {
      throw new Error('grid layout requires at least two Canvases');
    }
    const mainPercent = options.mainPercent ?? 60;
    if (mainPercent < 40 || mainPercent > 80) {
      throw new Error('mainPercent must be between 40 and 80');
    }
    for (const paneID of paneIDs) {
      if (!(await this.paneExists(paneID))) throw new Error(`tmux pane does not exist: ${paneID}`);
    }
    const focusPaneID = options.focusPaneID ?? paneIDs[0];
    if (!paneIDs.includes(focusPaneID)) throw new Error('focus must identify a visible Canvas');
    const previous = {
      layout: state.layout,
      paneIDs: [...state.visiblePaneIDs],
      focusPaneID: state.focusedPaneID,
      mainPercent: state.mainPercent,
    };
    try {
      await this.placeLayout(
        options.sessionID,
        options.layout,
        paneIDs,
        options.allPaneIDs,
        focusPaneID,
        mainPercent
      );
      return { layout: options.layout, visiblePaneIDs: paneIDs, focusedPaneID: focusPaneID };
    } catch (error) {
      const recoverable = previous.paneIDs.filter((paneID) => options.allPaneIDs.includes(paneID));
      if (recoverable.length) {
        try {
          await this.placeLayout(
            options.sessionID,
            previous.layout,
            recoverable,
            options.allPaneIDs,
            previous.focusPaneID && recoverable.includes(previous.focusPaneID)
              ? previous.focusPaneID
              : recoverable[0],
            previous.mainPercent
          );
        } catch {
          // Preserve the original tmux failure; all renderer processes remain alive in their windows.
        }
      }
      throw error;
    }
  }

  private async placeLayout(
    sessionID: string,
    layout: CanvasLayoutType,
    paneIDs: string[],
    allPaneIDs: string[],
    focusPaneID: string,
    mainPercent: number
  ): Promise<void> {
    const state = this.layoutState(sessionID);
    const hostPaneID = await this.getHostPaneID();
    const hostWindowID = await this.paneWindowID(hostPaneID);
    for (const paneID of allPaneIDs) {
      if (await this.paneExists(paneID)) await this.hidePane(paneID, hostWindowID);
    }
    await this.run(['join-pane', '-d', '-h', '-l', '67%', '-s', paneIDs[0], '-t', hostPaneID]);
    await this.joinLayoutPanes(layout, paneIDs, mainPercent);
    await this.run(['select-pane', '-t', focusPaneID]);
    state.visiblePaneIDs = new Set(paneIDs);
    state.focusedPaneID = focusPaneID;
    state.layout = layout;
    state.mainPercent = mainPercent;
  }

  private layoutState(sessionID: string): SessionLayoutState {
    let state = this.sessionLayouts.get(sessionID);
    if (!state) {
      state = {
        visiblePaneIDs: new Set<string>(),
        layout: 'single',
        mainPercent: 60,
      };
      this.sessionLayouts.set(sessionID, state);
    }
    return state;
  }

  private async deactivateOtherSessions(sessionID: string): Promise<void> {
    const hostPaneID = await this.getHostPaneID();
    const hostWindowID = await this.paneWindowID(hostPaneID);
    for (const [otherSessionID, state] of this.sessionLayouts) {
      if (otherSessionID === sessionID) continue;
      for (const paneID of state.visiblePaneIDs) {
        if (await this.paneExists(paneID)) await this.hidePane(paneID, hostWindowID);
      }
      state.visiblePaneIDs.clear();
      state.focusedPaneID = undefined;
      state.layout = 'single';
      state.mainPercent = 60;
    }
  }

  private async joinLayoutPanes(
    layout: CanvasLayoutType,
    paneIDs: string[],
    mainPercent: number
  ): Promise<void> {
    if (paneIDs.length < 2) return;
    const auxiliaryPercent = 100 - mainPercent;
    if (layout === 'columns') {
      for (let index = 1; index < paneIDs.length; index++) {
        await this.run(['join-pane', '-d', '-h', '-s', paneIDs[index], '-t', paneIDs[index - 1]]);
      }
      return;
    }
    if (layout === 'rows') {
      for (let index = 1; index < paneIDs.length; index++) {
        await this.run(['join-pane', '-d', '-v', '-s', paneIDs[index], '-t', paneIDs[index - 1]]);
      }
      return;
    }
    if (layout === 'main-left' || layout === 'main-right') {
      const args = ['join-pane', '-d', '-h', '-l', `${auxiliaryPercent}%`];
      if (layout === 'main-right') args.push('-b');
      await this.run([...args, '-s', paneIDs[1], '-t', paneIDs[0]]);
      for (let index = 2; index < paneIDs.length; index++) {
        await this.run(['join-pane', '-d', '-v', '-s', paneIDs[index], '-t', paneIDs[1]]);
      }
      return;
    }
    if (layout === 'main-top' || layout === 'main-bottom') {
      const args = ['join-pane', '-d', '-v', '-l', `${auxiliaryPercent}%`];
      if (layout === 'main-bottom') args.push('-b');
      await this.run([...args, '-s', paneIDs[1], '-t', paneIDs[0]]);
      for (let index = 2; index < paneIDs.length; index++) {
        await this.run(['join-pane', '-d', '-h', '-s', paneIDs[index], '-t', paneIDs[1]]);
      }
      return;
    }
    // Grid: split the first row horizontally, then split each column vertically.
    await this.run(['join-pane', '-d', '-h', '-s', paneIDs[1], '-t', paneIDs[0]]);
    if (paneIDs[2]) await this.run(['join-pane', '-d', '-v', '-s', paneIDs[2], '-t', paneIDs[0]]);
    if (paneIDs[3]) await this.run(['join-pane', '-d', '-v', '-s', paneIDs[3], '-t', paneIDs[1]]);
  }

  private async hidePane(paneID: string, hostWindowID: string): Promise<void> {
    if ((await this.paneWindowID(paneID)) !== hostWindowID) return;
    await this.run(['break-pane', '-d', '-s', paneID, '-n', 'canvas:hidden']);
  }

  private async paneWindowID(paneID: string): Promise<string> {
    const result = await this.run(['display-message', '-t', paneID, '-p', '#{window_id}']);
    return result.stdout.trim();
  }

  private async splitVisible(command: string, splitPercent: number): Promise<string> {
    const result = await this.run([
      'split-window',
      '-d',
      '-h',
      '-l',
      `${splitPercent}%`,
      '-P',
      '-F',
      '#{pane_id}',
      command,
    ]);
    return this.requirePaneID(result.stdout);
  }

  private async spawnHidden(command: string, title: string): Promise<string> {
    const sessionID = await this.getTmuxSessionID();
    const result = await this.run([
      'new-window',
      '-d',
      '-P',
      '-F',
      '#{pane_id}',
      '-n',
      title,
      '-t',
      `${sessionID}:`,
      command,
    ]);
    return this.requirePaneID(result.stdout);
  }

  private async paneExists(paneID: string): Promise<boolean> {
    try {
      const result = await this.run(['display-message', '-t', paneID, '-p', '#{pane_id}']);
      return result.stdout.trim() === paneID;
    } catch {
      return false;
    }
  }

  private async getHostPaneID(): Promise<string> {
    if (this.hostPaneID) return this.hostPaneID;

    if (process.env.TMUX_PANE) {
      this.hostPaneID = process.env.TMUX_PANE;
      return this.hostPaneID;
    }

    const result = await this.run(['display-message', '-p', '#{pane_id}']);
    this.hostPaneID = this.requirePaneID(result.stdout);
    return this.hostPaneID;
  }

  private async getTmuxSessionID(): Promise<string> {
    if (this.tmuxSessionID) return this.tmuxSessionID;

    const result = await this.run(['display-message', '-p', '#{session_id}']);
    const sessionID = result.stdout.trim();
    if (!sessionID) {
      throw new Error('Unable to determine tmux session id');
    }

    this.tmuxSessionID = sessionID;
    return sessionID;
  }

  private requirePaneID(stdout: string): string {
    const paneID = stdout.trim();
    if (!paneID.startsWith('%')) {
      throw new Error(`tmux did not return a pane id: ${stdout}`);
    }
    return paneID;
  }

  private ensureTmux(): void {
    if (!process.env.TMUX) {
      throw new Error('Canvas requires tmux. Please run the agent inside a tmux session.');
    }
  }

  private async run(args: string[]): Promise<TmuxRunResult> {
    const proc = Bun.spawn(['tmux', ...args], {
      stdout: 'pipe',
      stderr: 'pipe',
    });

    const [exitCode, stdout, stderr] = await Promise.all([
      proc.exited,
      streamText(proc.stdout),
      streamText(proc.stderr),
    ]);

    if (exitCode !== 0) {
      throw new Error(stderr.trim() || `tmux exited with code ${exitCode}`);
    }

    return { stdout, stderr };
  }
}
