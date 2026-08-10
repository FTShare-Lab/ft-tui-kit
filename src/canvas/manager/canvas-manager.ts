import path from 'path';
import { mkdir, rm } from 'fs/promises';
import {
  buildRendererCommand,
  type CanvasManifest,
  type CanvasManifestRegistry,
} from '../manifest.ts';
import {
  CANVAS_PROTOCOL_VERSION,
  createFrame,
  createId,
  type CanvasRegistryEntry,
  type HostControlFrame,
  type RendererCapabilities,
  type RendererControlFrame,
  type RendererEventFrame,
  type RendererLaunchConfig,
} from '../protocol.ts';
import { createRendererHostServer, type RendererHostServer } from '../ipc/host-server.ts';
import type { CanvasEventSink, CanvasSourceEvent, CanvasToolContext } from './event-sink.ts';
import { TmuxManager, type CanvasLayoutType } from './tmux.ts';

const CONFIG_APPLY_TIMEOUT_MS = 30_000;

interface ConfigApplyResult {
  success: boolean;
  error?: string;
}

interface ConfigApplyWaiter {
  requestId?: string;
  resolve: (result: ConfigApplyResult) => void;
  timeout: ReturnType<typeof setTimeout>;
}

interface CanvasRecord {
  id: string;
  kind: string;
  scenario: string;
  title?: string;
  status: 'starting' | 'ready' | 'closed' | 'error';
  runtimeDir: string;
  configFile: string;
  launchFile: string;
  controlSocketPath: string;
  eventSocketPath: string;
  paneID: string;
  sessionID: string;
  manifest: CanvasManifest;
  capabilities: RendererCapabilities;
  server: RendererHostServer;
  state: Map<string, unknown>;
  error?: string;
  configApplyWaiter?: ConfigApplyWaiter;
}

export interface CanvasCommandEvent {
  canvasID: string;
  kind: string;
  scenario: string;
  sessionID: string;
  name: string;
  data?: unknown;
}

export interface CanvasManagerOptions {
  pluginRoot: string;
  runtimeRoot: string;
  eventSink: CanvasEventSink;
  tmux: TmuxManager;
  manifests: CanvasManifestRegistry;
  onCommand?: (event: CanvasCommandEvent) => void | Promise<void>;
}

export interface SpawnCanvasInput {
  kind: string;
  scenario?: string;
  config: string;
  title?: string;
  activate?: boolean;
  context: CanvasToolContext;
}

export interface SpawnInternalCanvasInput {
  kind: string;
  scenario?: string;
  config: unknown;
  title?: string;
  activate?: boolean;
  sessionID: string;
  agent?: string;
}

interface SpawnOwnedCanvasInput {
  kind: string;
  scenario?: string;
  config: string;
  title?: string;
  activate?: boolean;
  owner: {
    sessionID: string;
    agent?: string;
    workspace?: string;
  };
  internal: boolean;
}

export interface CanvasLayoutInput {
  layout: CanvasLayoutType;
  ids: string[];
  mainPercent?: number;
  focus?: string;
  context: CanvasToolContext;
}

export class CanvasManager {
  private readonly records = new Map<string, CanvasRecord>();

  constructor(private readonly options: CanvasManagerOptions) {}

  availableRenderers(): Record<string, unknown> {
    return {
      success: true,
      renderers: this.options.manifests.publicList().map((manifest) => ({
        name: manifest.name,
        description: manifest.description,
        defaultScenario: manifest.defaultScenario,
        scenarios: manifest.scenarios,
        capabilities: manifest.capabilities,
      })),
    };
  }

  async spawn(input: SpawnCanvasInput): Promise<Record<string, unknown>> {
    this.options.eventSink.rememberToolContext(input.context);

    return this.spawnOwned({
      kind: input.kind,
      scenario: input.scenario,
      config: input.config,
      title: input.title,
      activate: input.activate,
      owner: input.context,
      internal: false,
    });
  }

  async spawnInternal(input: SpawnInternalCanvasInput): Promise<Record<string, unknown>> {
    return this.spawnOwned({
      kind: input.kind,
      scenario: input.scenario,
      config: JSON.stringify(input.config),
      title: input.title,
      activate: input.activate,
      owner: {
        sessionID: input.sessionID,
        agent: input.agent,
      },
      internal: true,
    });
  }

  private async spawnOwned(input: SpawnOwnedCanvasInput): Promise<Record<string, unknown>> {
    const manifest = this.options.manifests.require(input.kind);
    if (input.internal !== manifest.internalOnly) {
      if (manifest.internalOnly) {
        throw new Error(`Canvas renderer is reserved for plugin-internal use: ${manifest.name}`);
      }
      throw new Error(`Canvas renderer is not marked for plugin-internal use: ${manifest.name}`);
    }
    const scenario = input.scenario ?? manifest.defaultScenario;
    this.validateScenario(manifest, scenario);

    const id = this.createID(manifest.name);
    const runtimeDir = path.join(this.options.runtimeRoot, id);
    const controlSocketPath = path.join(runtimeDir, 'control.sock');
    const eventSocketPath = path.join(runtimeDir, 'event.sock');
    const configFile = path.join(runtimeDir, 'config.json');
    const launchFile = path.join(runtimeDir, 'launch.json');
    const token = createId('token');
    const config = this.parseConfig(input.config);

    await mkdir(runtimeDir, { recursive: true, mode: 0o700 });
    await Bun.write(configFile, JSON.stringify(config, null, 2));

    const launch: RendererLaunchConfig = {
      version: CANVAS_PROTOCOL_VERSION,
      widgetId: id,
      kind: manifest.name,
      scenario,
      title: input.title,
      token,
      runtimeDir,
      controlSocketPath,
      eventSocketPath,
      configPath: configFile,
      manifest: {
        name: manifest.name,
        description: manifest.description,
        defaultScenario: manifest.defaultScenario,
        capabilities: manifest.capabilities,
      },
    };

    await Bun.write(launchFile, JSON.stringify(launch, null, 2));

    let record: CanvasRecord | undefined;
    const pendingMessages: Array<
      | { kind: 'control'; frame: RendererControlFrame }
      | { kind: 'event'; frame: RendererEventFrame }
      | { kind: 'disconnect' }
      | { kind: 'error'; error: Error }
    > = [];
    const server = await createRendererHostServer({
      widgetId: id,
      token,
      controlSocketPath,
      eventSocketPath,
      launch,
      config,
      onControlMessage: (frame) => {
        if (record) return this.routeControlMessage(record, frame);
        pendingMessages.push({ kind: 'control', frame });
      },
      onEventMessage: (frame) => {
        if (record) return this.routeEventMessage(record, frame);
        pendingMessages.push({ kind: 'event', frame });
      },
      onDisconnect: async () => {
        if (!record) {
          pendingMessages.push({ kind: 'disconnect' });
          return;
        }
        record.status = 'closed';
        this.failConfigApply(record, 'Renderer disconnected before applying the configuration');
        await this.broadcastRegistry(record.sessionID);
      },
      onError: (error) => {
        if (!record) {
          pendingMessages.push({ kind: 'error', error });
          return;
        }
        record.status = 'error';
        record.error = error.message;
        this.failConfigApply(record, error.message);
      },
    });

    try {
      const command = buildRendererCommand(manifest, {
        pluginRoot: this.options.pluginRoot,
        launchFile,
        widgetId: id,
        kind: manifest.name,
        scenario,
        runtimeDir,
      });

      const tmux = await this.options.tmux.spawn({
        command,
        title: `canvas:${manifest.name}`,
        sessionID: input.owner.sessionID,
        activate: input.activate,
      });

      record = {
        id,
        kind: manifest.name,
        scenario,
        title: input.title,
        status: 'starting',
        runtimeDir,
        configFile,
        launchFile,
        controlSocketPath,
        eventSocketPath,
        paneID: tmux.paneID,
        sessionID: input.owner.sessionID,
        manifest,
        capabilities: manifest.capabilities,
        server,
        state: new Map<string, unknown>(),
      };

      this.records.set(id, record);
      const configApply = this.beginConfigApply(record);
      for (const message of pendingMessages) {
        if (message.kind === 'control') {
          await this.routeControlMessage(record, message.frame);
          continue;
        }
        if (message.kind === 'event') {
          await this.routeEventMessage(record, message.frame);
          continue;
        }
        if (message.kind === 'disconnect') {
          record.status = 'closed';
          this.failConfigApply(record, 'Renderer disconnected before applying the configuration');
          continue;
        }
        record.status = 'error';
        record.error = message.error.message;
        this.failConfigApply(record, message.error.message);
      }
      pendingMessages.length = 0;
      await this.broadcastRegistry();

      const applied = await configApply;
      if (!applied.success) {
        return {
          success: false,
          id,
          kind: record.kind,
          scenario,
          status: record.status,
          error: applied.error ?? record.error ?? 'Renderer rejected the configuration',
        };
      }

      return {
        success: true,
        id,
        kind: record.kind,
        scenario,
        status: record.status,
        paneID: tmux.paneID,
        visible: tmux.visible,
        launchFile,
        controlSocketPath,
        eventSocketPath,
      };
    } catch (error) {
      server.close('spawn failed');
      await rm(runtimeDir, { recursive: true, force: true });
      throw error;
    }
  }

  async update(
    id: string,
    configText: string,
    context: CanvasToolContext
  ): Promise<Record<string, unknown>> {
    this.options.eventSink.rememberToolContext(context);
    const record = this.requireOwned(id, context.sessionID);
    const config = this.parseConfig(configText);
    if (!record.server.isControlConnected()) {
      return {
        success: false,
        id,
        error: 'Renderer control channel is not connected',
      };
    }

    await Bun.write(record.configFile, JSON.stringify(config, null, 2));
    const requestId = createId('config');
    const configApply = this.beginConfigApply(record, requestId);
    record.status = 'starting';
    record.error = undefined;
    record.server.sendControl(
      createFrame('control', record.id, 'update', { config }, requestId) as HostControlFrame
    );
    await this.broadcastRegistry(record.sessionID);

    const applied = await configApply;
    return {
      success: applied.success,
      id,
      status: record.status,
      ...(applied.error ? { error: applied.error } : {}),
    };
  }

  async selection(id: string, context: CanvasToolContext): Promise<Record<string, unknown>> {
    const record = this.requireOwned(id, context.sessionID);
    return {
      success: true,
      id,
      selection: await record.server.request('selection'),
    };
  }

  async content(id: string, context: CanvasToolContext): Promise<Record<string, unknown>> {
    const record = this.requireOwned(id, context.sessionID);
    return {
      success: true,
      id,
      content: await record.server.request('content'),
    };
  }

  async state(
    id: string,
    context: CanvasToolContext,
    key?: string
  ): Promise<Record<string, unknown>> {
    const record = this.requireOwned(id, context.sessionID);
    if (record.server.isControlConnected()) {
      return {
        success: true,
        id,
        state: await record.server.request('state', key ? { key } : {}),
      };
    }

    return {
      success: true,
      id,
      state: key ? record.state.get(key) : Object.fromEntries(record.state),
    };
  }

  list(sessionID?: string): Record<string, unknown> {
    const widgets = this.registryEntries(sessionID, false);
    const active = widgets.find((item) => item.active);
    const visibleIDs = widgets.filter((item) => item.visible).map((item) => item.id);
    return {
      success: true,
      activeID: active?.id,
      focusedID: active?.id,
      visibleIDs,
      layout: sessionID ? this.options.tmux.currentLayout(sessionID) : 'single',
      widgets,
    };
  }

  async layout(input: CanvasLayoutInput): Promise<Record<string, unknown>> {
    this.options.eventSink.rememberToolContext(input.context);
    if (!input.ids.length) throw new Error('canvas_layout requires at least one Canvas ID');
    if (new Set(input.ids).size !== input.ids.length) {
      throw new Error('canvas_layout ids must not contain duplicates');
    }
    const records = input.ids.map((id) => this.requireOwned(id, input.context.sessionID));
    const focus = input.focus
      ? this.requireOwned(input.focus, input.context.sessionID)
      : records[0];
    if (!records.some((record) => record.id === focus.id)) {
      throw new Error('canvas_layout focus must also appear in ids');
    }
    const sessionRecords = this.publicRecordsForSession(input.context.sessionID);
    const allSessionRecords = this.recordsForSession(input.context.sessionID);
    const result = await this.options.tmux.applyLayout({
      sessionID: input.context.sessionID,
      layout: input.layout,
      paneIDs: records.map((record) => record.paneID),
      allPaneIDs: allSessionRecords.map((record) => record.paneID),
      focusPaneID: focus.paneID,
      mainPercent: input.mainPercent,
    });
    await this.broadcastRegistry();
    const visiblePaneIDs = new Set(result.visiblePaneIDs);
    return {
      success: true,
      layout: result.layout,
      visibleIDs: records
        .filter((record) => visiblePaneIDs.has(record.paneID))
        .map((record) => record.id),
      focusedID: sessionRecords.find((record) => record.paneID === result.focusedPaneID)?.id,
      hiddenIDs: sessionRecords
        .filter((record) => !visiblePaneIDs.has(record.paneID))
        .map((record) => record.id),
      panes: sessionRecords.map((record) => ({
        id: record.id,
        visible: visiblePaneIDs.has(record.paneID),
        focused: record.paneID === result.focusedPaneID,
      })),
    };
  }

  async switch(id: string, context: CanvasToolContext): Promise<Record<string, unknown>> {
    const record = this.requireOwned(id, context.sessionID);
    await this.switchForSession(record.id, context.sessionID);
    return { success: true, id, activeID: id };
  }

  async next(sessionID: string): Promise<Record<string, unknown>> {
    const records = this.publicRecordsForSession(sessionID);
    const nextPaneID = await this.options.tmux.next(
      sessionID,
      records.map((record) => record.paneID)
    );
    const active = nextPaneID ? records.find((record) => record.paneID === nextPaneID) : undefined;

    await this.broadcastRegistry();
    return { success: true, activeID: active?.id };
  }

  async close(id: string, context: CanvasToolContext): Promise<Record<string, unknown>> {
    const record = this.requireOwned(id, context.sessionID);
    await this.closeRecord(record, 'Closed by Canvas tool');
    await this.broadcastRegistry();
    return { success: true, id, closed: true };
  }

  async closeInternal(id: string, sessionID: string, reason: string): Promise<void> {
    const record = this.requireOwnedBySession(id, sessionID);
    if (!record.manifest.internalOnly) {
      throw new Error(`Canvas is not plugin-internal: ${id}`);
    }
    await this.closeRecord(record, reason);
    await this.broadcastRegistry(sessionID);
  }

  async closeSession(sessionID: string): Promise<void> {
    const records = this.recordsForSession(sessionID);
    for (const record of records) {
      await this.closeRecord(record, 'Agent session closed');
    }
    this.options.tmux.forgetSession(sessionID);
    this.options.eventSink.clear(sessionID);
    await this.broadcastRegistry();
  }

  async dispose(): Promise<void> {
    for (const record of Array.from(this.records.values())) {
      await this.closeRecord(record, 'Canvas host disposed');
    }
  }

  private async routeControlMessage(
    record: CanvasRecord,
    frame: RendererControlFrame
  ): Promise<void> {
    switch (frame.type) {
      case 'hello':
        return;
      case 'ready':
        record.status = 'ready';
        record.error = undefined;
        record.title = frame.payload.title ?? record.title;
        record.capabilities = frame.payload.capabilities ?? record.capabilities;
        this.completeConfigApply(record, frame.requestId, { success: true });
        await this.broadcastRegistry(record.sessionID);
        return;
      case 'capabilities':
        record.capabilities = frame.payload;
        await this.broadcastRegistry(record.sessionID);
        return;
      case 'error':
        record.status = 'error';
        record.error = frame.payload.message;
        this.completeConfigApply(record, frame.requestId, {
          success: false,
          error: frame.payload.message,
        });
        await this.broadcastRegistry(record.sessionID);
        return;
      case 'pong':
      case 'rpc.response':
        return;
    }
  }

  private async routeEventMessage(record: CanvasRecord, frame: RendererEventFrame): Promise<void> {
    switch (frame.type) {
      case 'hello':
        return;
      case 'state':
        record.state.set(frame.payload.key ?? frame.payload.label ?? 'state', frame.payload.data);
        return;
      case 'selection':
        await this.handleSelection(record, frame);
        return;
      case 'context':
        await this.handleContext(record, frame);
        return;
      case 'action':
        await this.handleAction(record, frame);
        return;
      case 'artifact':
        await this.handleArtifact(record, frame);
        return;
      case 'command':
        await this.handleCommand(record, frame);
        return;
      case 'control':
        await this.routeRendererControl(record, frame.payload.command, frame.payload.targetId);
        return;
      case 'cancelled':
        record.state.set('cancelled', { reason: frame.payload.reason, time: Date.now() });
        return;
      case 'error':
        record.status = 'error';
        record.error = frame.payload.message;
        this.failConfigApply(record, frame.payload.message);
        await this.broadcastRegistry(record.sessionID);
        return;
      case 'log':
        record.state.set('lastLog', frame.payload);
        return;
    }
  }

  private async handleSelection(
    record: CanvasRecord,
    frame: Extract<RendererEventFrame, { type: 'selection' }>
  ): Promise<void> {
    record.state.set('selection', frame.payload.data);
    if (!frame.payload.delivery) return;

    if (frame.payload.delivery === 'context') {
      this.options.eventSink.attachContext(record.sessionID, {
        sourceID: record.id,
        sourceEvent: 'selection',
        label: frame.payload.label ?? 'selection',
        text: this.textForEvent(record, 'selection', frame.payload.text, frame.payload.data),
        data: frame.payload.data,
        createdAt: Date.now(),
      });
      return;
    }

    await this.enqueuePrompt(
      record,
      'selection',
      frame.payload.label ?? 'selection',
      this.promptForEvent(record, frame.payload)
    );
  }

  private async handleContext(
    record: CanvasRecord,
    frame: Extract<RendererEventFrame, { type: 'context' }>
  ): Promise<void> {
    if (frame.payload.delivery === 'queue' || frame.payload.delivery === 'steer') {
      await this.enqueuePrompt(
        record,
        'context',
        frame.payload.label,
        this.promptForEvent(record, frame.payload)
      );
      return;
    }

    this.options.eventSink.attachContext(record.sessionID, {
      sourceID: record.id,
      sourceEvent: 'context',
      label: frame.payload.label,
      text: this.textForEvent(
        record,
        frame.payload.label ?? 'context',
        frame.payload.text,
        frame.payload.data
      ),
      data: frame.payload.data,
      createdAt: Date.now(),
    });
  }

  private async handleAction(
    record: CanvasRecord,
    frame: Extract<RendererEventFrame, { type: 'action' }>
  ): Promise<void> {
    await this.enqueuePrompt(
      record,
      'action',
      frame.payload.label,
      this.promptForEvent(record, frame.payload)
    );
  }

  private async handleArtifact(
    record: CanvasRecord,
    frame: Extract<RendererEventFrame, { type: 'artifact' }>
  ): Promise<void> {
    record.state.set(`artifact:${frame.payload.label ?? frame.id}`, frame.payload);

    if (!frame.payload.delivery) return;
    if (frame.payload.delivery === 'context') {
      this.options.eventSink.attachContext(record.sessionID, {
        sourceID: record.id,
        sourceEvent: 'artifact',
        label: frame.payload.label ?? 'artifact',
        text: this.textForEvent(record, 'artifact', frame.payload.text, frame.payload),
        data: frame.payload,
        createdAt: Date.now(),
      });
      return;
    }

    await this.enqueuePrompt(
      record,
      'artifact',
      frame.payload.label ?? 'artifact',
      this.promptForEvent(record, frame.payload)
    );
  }

  private async handleCommand(
    record: CanvasRecord,
    frame: Extract<RendererEventFrame, { type: 'command' }>
  ): Promise<void> {
    if (!record.manifest.internalOnly) {
      throw new Error(`Canvas commands are restricted to plugin-internal renderers: ${record.id}`);
    }
    if (!record.capabilities.command) {
      throw new Error(`Canvas did not declare the command capability: ${record.id}`);
    }
    if (!this.options.onCommand) {
      throw new Error(`No command handler is registered for Canvas ${record.id}`);
    }

    await this.options.onCommand({
      canvasID: record.id,
      kind: record.kind,
      scenario: record.scenario,
      sessionID: record.sessionID,
      name: frame.payload.name,
      data: frame.payload.data,
    });
  }

  private async routeRendererControl(
    record: CanvasRecord,
    command: 'switch' | 'next' | 'close',
    targetId?: string
  ): Promise<void> {
    if (command === 'switch' && targetId) {
      await this.switchForSession(targetId, record.sessionID);
      return;
    }

    if (command === 'next') {
      await this.next(record.sessionID);
      return;
    }

    if (command === 'close') {
      const target = targetId ? this.requireOwnedBySession(targetId, record.sessionID) : record;
      await this.closeRecord(target, 'Closed by renderer');
      await this.broadcastRegistry(record.sessionID);
    }
  }

  private async switchForSession(id: string, sessionID: string): Promise<void> {
    const record = this.requireOwnedBySession(id, sessionID);
    await this.options.tmux.switchTo(sessionID, record.paneID);
    await this.broadcastRegistry();
  }

  private async closeRecord(record: CanvasRecord, reason: string): Promise<void> {
    if (!this.records.has(record.id)) return;

    const visiblePaneIDs = new Set(this.options.tmux.visiblePaneIDs(record.sessionID));
    const candidates = this.recordsForSession(record.sessionID).filter(
      (item) => item.id !== record.id
    );
    const fallback = candidates.find((item) => visiblePaneIDs.has(item.paneID)) ?? candidates[0];
    this.failConfigApply(record, reason);
    this.records.delete(record.id);
    record.status = 'closed';
    record.server.close(reason);
    await this.options.tmux.closePane(record.sessionID, record.paneID, fallback?.paneID);
    await rm(record.runtimeDir, { recursive: true, force: true });
  }

  private async broadcastRegistry(sessionID?: string): Promise<void> {
    const sessions = sessionID
      ? [sessionID]
      : Array.from(new Set(Array.from(this.records.values()).map((record) => record.sessionID)));

    for (const id of sessions) {
      const widgets = this.registryEntries(id);
      const activeID = widgets.find((item) => item.active)?.id;
      const visiblePaneIDs = new Set(this.options.tmux.visiblePaneIDs(id));
      const focusedPaneID = this.options.tmux.activePaneID(id);
      for (const record of this.recordsForSession(id)) {
        record.server.sendControl(
          createFrame('control', record.id, 'registry', {
            widgets,
            activeId: activeID,
          }) as HostControlFrame
        );
        record.server.sendControl(
          createFrame('control', record.id, 'focus', {
            active: visiblePaneIDs.has(record.paneID),
            focused: focusedPaneID === record.paneID,
          }) as HostControlFrame
        );
      }
    }
  }

  private registryEntries(
    sessionID?: string,
    includeInternal: boolean = true
  ): CanvasRegistryEntry[] {
    const visiblePaneIDs = new Set(sessionID ? this.options.tmux.visiblePaneIDs(sessionID) : []);
    const focusedPaneID = sessionID ? this.options.tmux.activePaneID(sessionID) : undefined;
    return Array.from(this.records.values())
      .filter((record) => !sessionID || record.sessionID === sessionID)
      .filter((record) => includeInternal || !record.manifest.internalOnly)
      .map((record) => {
        const focused = focusedPaneID === record.paneID;
        const visible = visiblePaneIDs.has(record.paneID);
        return {
          id: record.id,
          kind: record.kind,
          scenario: record.scenario,
          title: record.title,
          status: record.status,
          active: focused,
          visible,
          focused,
          capabilities: record.capabilities,
        };
      });
  }

  private recordsForSession(sessionID: string): CanvasRecord[] {
    return Array.from(this.records.values()).filter((record) => record.sessionID === sessionID);
  }

  private publicRecordsForSession(sessionID: string): CanvasRecord[] {
    return this.recordsForSession(sessionID).filter((record) => !record.manifest.internalOnly);
  }

  private requireOwned(id: string, sessionID: string): CanvasRecord {
    const record = this.requireOwnedBySession(id, sessionID);
    if (record.manifest.internalOnly) {
      throw new Error(`Canvas is managed internally by the plugin: ${id}`);
    }
    return record;
  }

  private requireOwnedBySession(id: string, sessionID: string): CanvasRecord {
    const record = this.records.get(id);
    if (!record || record.sessionID !== sessionID) {
      throw new Error(`Canvas not found for this session: ${id}`);
    }
    return record;
  }

  private parseConfig(config: string): unknown {
    try {
      return JSON.parse(config);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      throw new Error(`Invalid config JSON: ${message}`);
    }
  }

  private validateScenario(manifest: CanvasManifest, scenario: string): void {
    if (!manifest.scenarios.length || manifest.scenarios.includes(scenario)) return;
    throw new Error(
      `Unknown scenario ${scenario} for renderer ${manifest.name}. Known scenarios: ${manifest.scenarios.join(', ')}`
    );
  }

  private createID(kind: string): string {
    return `${kind}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  }

  private beginConfigApply(record: CanvasRecord, requestId?: string): Promise<ConfigApplyResult> {
    if (record.configApplyWaiter) {
      throw new Error(`Canvas ${record.id} is already applying another configuration`);
    }

    return new Promise((resolve) => {
      const timeout = setTimeout(() => {
        if (record.configApplyWaiter?.requestId !== requestId) return;

        record.configApplyWaiter = undefined;
        const error = `Timed out after ${CONFIG_APPLY_TIMEOUT_MS}ms waiting for renderer to apply configuration`;
        record.status = 'error';
        record.error = error;
        resolve({ success: false, error });
        void this.broadcastRegistry(record.sessionID);
      }, CONFIG_APPLY_TIMEOUT_MS);

      record.configApplyWaiter = { requestId, resolve, timeout };
    });
  }

  private completeConfigApply(
    record: CanvasRecord,
    requestId: string | undefined,
    result: ConfigApplyResult
  ): void {
    const waiter = record.configApplyWaiter;
    if (!waiter) return;
    if (waiter.requestId !== undefined && waiter.requestId !== requestId) return;

    clearTimeout(waiter.timeout);
    record.configApplyWaiter = undefined;
    waiter.resolve(result);
  }

  private failConfigApply(record: CanvasRecord, error: string): void {
    const waiter = record.configApplyWaiter;
    if (!waiter) return;

    clearTimeout(waiter.timeout);
    record.configApplyWaiter = undefined;
    waiter.resolve({ success: false, error });
  }

  private async enqueuePrompt(
    record: CanvasRecord,
    sourceEvent: CanvasSourceEvent,
    label: string | undefined,
    prompt: string
  ): Promise<void> {
    await this.options.eventSink.enqueueAction(record.sessionID, {
      sourceID: record.id,
      sourceEvent,
      label,
      prompt,
      createdAt: Date.now(),
    });
  }

  private promptForEvent(
    record: CanvasRecord,
    payload: { prompt?: string; text?: string; data?: unknown; label?: string }
  ): string {
    if (payload.prompt) return payload.prompt;
    return this.textForEvent(record, payload.label ?? 'event', payload.text, payload.data);
  }

  private textForEvent(record: CanvasRecord, label: string, text?: string, data?: unknown): string {
    const sections = [
      `Canvas event from ${record.id} (${record.kind}/${record.scenario})`,
      `Label: ${label}`,
      text,
    ];
    if (data !== undefined) {
      sections.push('Data:', JSON.stringify(data, null, 2));
    }
    return sections.filter(Boolean).join('\n');
  }
}
