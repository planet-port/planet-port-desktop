import { createContext, useContext, useMemo, type ReactNode } from 'react';
import { Toaster as ToastLayer, pushToast } from './ui-x/Toaster';

/// 全局 toast 薄层:useToast()/ToastProvider 对外签名与原实现一致(消费方
/// 零改动),入队与渲染全部下沉到 ui-x/Toaster(sonner 封装)。

const Ctx = createContext<{ ok: (t: string) => void; error: (t: string) => void }>({ ok: () => {}, error: () => {} });

export const useToast = () => useContext(Ctx);

export function ToastProvider({ children }: { children: ReactNode }) {
  const value = useMemo(
    () => ({ ok: (t: string) => pushToast('ok', t), error: (t: string) => pushToast('error', t) }),
    [],
  );
  return (
    <Ctx.Provider value={value}>
      {children}
      <ToastLayer />
    </Ctx.Provider>
  );
}
