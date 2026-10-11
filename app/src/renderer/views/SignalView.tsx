import { SignalPathPanel } from '../components/SignalPathPanel'
import { usePlayer } from '../stores/player'

export function SignalView() {
  const signal = usePlayer((p) => p.signal)
  return <SignalPathPanel signal={signal} />
}
