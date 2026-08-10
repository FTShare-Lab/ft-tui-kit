import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Box, Text, useApp, useInput, useStdout } from 'ink';
import type { RendererLaunchConfig } from '../../src/canvas/protocol.ts';
import { useIPC } from '../_sdk/ipc/use-ipc.ts';
import {
  isMouseControlInput,
  stripMouseControlSequences,
  useMouse,
  type MouseEvent,
} from '../_sdk/use-mouse.ts';

interface SocialPostCardConfig {
  post: string;
  generatedAt: string;
  conversationMinutes: number;
}

interface SocialPostCardProps {
  id: string;
  config?: unknown;
  launch?: RendererLaunchConfig;
  scenario?: string;
}

type Action = 'save' | 'cancel';

const CARD_COLOR = '#7dd3fc';
const MUTED_COLOR = '#94a3b8';
const SAVE_COMMAND = 'social-post.save';
const CANCEL_COMMAND = 'social-post.cancel';

export function SocialPostCard({
  config: initialConfig,
  launch,
  scenario = 'review',
}: SocialPostCardProps) {
  const { exit } = useApp();
  const { stdout } = useStdout();
  const [config, setConfig] = useState(() => parseConfig(initialConfig));
  const [selectedAction, setSelectedAction] = useState<Action>('save');
  const [submitting, setSubmitting] = useState(false);
  const [dimensions, setDimensions] = useState(() => readDimensions(stdout));
  const resetTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const ipc = useIPC({
    launch,
    scenario,
    title: '技术动态草稿',
    onClose: exit,
    onUpdate: (nextConfig) => setConfig(parseConfig(nextConfig)),
  });

  useEffect(() => {
    const updateDimensions = () => setDimensions(readDimensions(stdout));
    stdout?.on('resize', updateDimensions);
    return () => {
      stdout?.off('resize', updateDimensions);
    };
  }, [stdout]);

  useEffect(
    () => () => {
      if (resetTimer.current) clearTimeout(resetTimer.current);
    },
    []
  );

  const activate = useCallback(
    (action: Action) => {
      if (submitting) return;
      setSelectedAction(action);
      setSubmitting(true);
      ipc.sendCommand(action === 'save' ? SAVE_COMMAND : CANCEL_COMMAND);

      if (!launch) {
        exit();
        return;
      }

      resetTimer.current = setTimeout(() => setSubmitting(false), 2_000);
    },
    [exit, ipc, launch, submitting]
  );

  useInput((input, key) => {
    const safeInput = stripMouseControlSequences(input);
    if (input && !safeInput && isMouseControlInput(input)) return;
    if (submitting) return;

    if (key.leftArrow || key.rightArrow || key.tab) {
      setSelectedAction((current) => (current === 'save' ? 'cancel' : 'save'));
      return;
    }
    if (key.return) {
      activate(selectedAction);
      return;
    }
    if (safeInput.toLowerCase() === 's') {
      activate('save');
      return;
    }
    if (safeInput.toLowerCase() === 'c' || key.escape) {
      activate('cancel');
    }
  });

  const handleMouseClick = useCallback(
    (event: MouseEvent) => {
      if (submitting || event.button !== 0 || event.y < dimensions.height - 3) return;
      const cancelStart = dimensions.width - 11;
      const saveStart = cancelStart - 10;
      if (event.x >= cancelStart) {
        activate('cancel');
      } else if (event.x >= saveStart) {
        activate('save');
      }
    },
    [activate, dimensions.height, dimensions.width, submitting]
  );

  useMouse({ enabled: true, tracking: 'button', onClick: handleMouseClick });

  const visiblePost = useMemo(
    () => fitPost(config.post, dimensions.width, dimensions.height),
    [config.post, dimensions.height, dimensions.width]
  );
  const generatedTime = formatGeneratedTime(config.generatedAt);

  return (
    <Box
      width={dimensions.width}
      height={dimensions.height}
      flexDirection="column"
      borderStyle="round"
      borderColor={CARD_COLOR}
      paddingX={2}
      paddingY={1}
    >
      <Box justifyContent="space-between">
        <Text bold color={CARD_COLOR}>
          技术动态草稿
        </Text>
        <Text color="black" backgroundColor={CARD_COLOR} bold>
          {' AUTO · REVIEW '}
        </Text>
      </Box>

      <Box marginTop={1}>
        <Text color={MUTED_COLOR}>
          本轮对话用时 {config.conversationMinutes.toFixed(1)} 分钟 · {generatedTime}
        </Text>
      </Box>

      <Box marginTop={1} flexGrow={1} flexDirection="column">
        <Text wrap="wrap">{visiblePost}</Text>
      </Box>

      <Box marginTop={1} justifyContent="space-between" alignItems="center">
        <Text color={MUTED_COLOR}>
          {submitting
            ? statusText(selectedAction)
            : dimensions.width >= 62
              ? '发送将保存为本地 Markdown · ←/→ 选择'
              : '←/→ 选择 · Enter 确认'}
        </Text>
        <Box>
          <ActionButton label="发送" active={selectedAction === 'save'} disabled={submitting} />
          <Box marginLeft={1}>
            <ActionButton label="取消" active={selectedAction === 'cancel'} disabled={submitting} />
          </Box>
        </Box>
      </Box>
    </Box>
  );
}

function ActionButton({
  label,
  active,
  disabled,
}: {
  label: string;
  active: boolean;
  disabled: boolean;
}) {
  const color = disabled ? MUTED_COLOR : active ? 'black' : CARD_COLOR;
  const backgroundColor = !disabled && active ? CARD_COLOR : undefined;
  return (
    <Text bold={!disabled && active} color={color} backgroundColor={backgroundColor}>
      {`[ ${label} ]`}
    </Text>
  );
}

function parseConfig(value: unknown): SocialPostCardConfig {
  if (!value || typeof value !== 'object') {
    throw new Error('social-post-card config must be an object');
  }

  const candidate = value as Record<string, unknown>;
  if (typeof candidate.post !== 'string' || !candidate.post.trim()) {
    throw new Error('social-post-card config.post must be a non-empty string');
  }
  if (typeof candidate.generatedAt !== 'string' || !candidate.generatedAt.trim()) {
    throw new Error('social-post-card config.generatedAt must be an ISO timestamp string');
  }
  if (
    typeof candidate.conversationMinutes !== 'number' ||
    !Number.isFinite(candidate.conversationMinutes) ||
    candidate.conversationMinutes < 0
  ) {
    throw new Error('social-post-card config.conversationMinutes must be a non-negative number');
  }

  return {
    post: candidate.post.trim(),
    generatedAt: candidate.generatedAt,
    conversationMinutes: candidate.conversationMinutes,
  };
}

function readDimensions(stdout: { columns?: number; rows?: number } | undefined): {
  width: number;
  height: number;
} {
  return {
    width: Math.max(36, stdout?.columns ?? 80),
    height: Math.max(12, stdout?.rows ?? 28),
  };
}

function fitPost(post: string, width: number, height: number): string {
  const availableRows = Math.max(3, height - 9);
  const conservativeColumns = Math.max(12, Math.floor((width - 6) / 2));
  const maxCharacters = availableRows * conservativeColumns;
  if (post.length <= maxCharacters) return post;
  return `${post.slice(0, Math.max(1, maxCharacters - 1)).trimEnd()}…`;
}

function formatGeneratedTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
}

function statusText(action: Action): string {
  return action === 'save' ? '正在保存到本地 Markdown…' : '正在关闭草稿…';
}

export default SocialPostCard;
