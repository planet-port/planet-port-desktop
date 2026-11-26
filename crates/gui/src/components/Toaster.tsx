import { createContext, useContext, useMemo, type ReactNode } from 'react';
import { Toaster as ToastLayer, pushToast, type ToastAction } from './ui-x/Toaster';

/// 全局 toast 薄层:useToast()/ToastProvider 对外签名与原实现一致(消费方
/// 零改动),入队与渲染全部下沉到 ui-x/Toaster(sonner 封装)。
/// action 可选:传入即在条目内渲染动作按钮(启动版本检测的「打开设置」入口)

const Ctx = createContext<{ ok: (t: string, action?: ToastAction) => void; error: (t: string, action?: ToastAction) => void }>({
  ok: () => {},
  error: () => {},
});

export const useToast = () => useContext(Ctx);

export function ToastProvider({ children }: { children: ReactNode }) {
  const value = useMemo(
    () => ({
      ok: (t: string, action?: ToastAction) => pushToast('ok', t, action),
      error: (t: string, action?: ToastAction) => pushToast('error', t, action),
    }),
    [],
  );
  return (
    <Ctx.Provider value={value}>
      {children}
      <ToastLayer />
    </Ctx.Provider>
  );
}
