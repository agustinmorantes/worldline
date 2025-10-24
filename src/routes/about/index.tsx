import { createFileRoute } from '@tanstack/react-router'
import {ReactElement} from "react";

export const Route = createFileRoute('/about/')({
  component: Index,
})

function Index(): ReactElement {
  return <div className="p-2">Hello from About!</div>
}
