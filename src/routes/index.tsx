import {createFileRoute, Link} from '@tanstack/react-router'
import {ReactElement, useState} from "react";
import reactLogo from "@/assets/react.svg";
import { invoke } from "@tauri-apps/api/core";
import classes from "./index.module.scss";
import {Button} from "@/components/ui/button.tsx";
import {Input} from "@/components/ui/input.tsx";

export const Route = createFileRoute('/')({
  component: Index,
})

function Index(): ReactElement {
  const [greetMsg, setGreetMsg] = useState("");
  const [name, setName] = useState("");

  async function greet(): Promise<void> {
    // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
    setGreetMsg(await invoke("greet", { name }));
  }

  return (
    <main className={classes.container}>
      <h1>Welcome to Tauri + React</h1>

      <div className={classes.row}>
        <a href="https://vite.dev" target="_blank" rel="noreferrer noopener">
          <img src="/vite.svg" className={`${classes.logo} ${classes.vite}`} alt="Vite logo" />
        </a>
        <a href="https://tauri.app" target="_blank" rel="noreferrer noopener">
          <img src="/tauri.svg" className={`${classes.logo} ${classes.tauri}`} alt="Tauri logo" />
        </a>
        <a href="https://react.dev" target="_blank" rel="noreferrer noopener">
          <img src={reactLogo} className={`${classes.logo} ${classes.react}`} alt="React logo" />
        </a>
      </div>
      <p>Click on the Tauri, Vite, and React logos to learn more.</p>

      <Link to="/about">About</Link>

      <form
        className={classes.form}
        onSubmit={(e) => {
          e.preventDefault();
          void greet();
        }}
      >
        <Input
          id="greet-input"
          onChange={(e) => { setName(e.currentTarget.value)}}
          placeholder="Enter a name..."
        />
        <Button type="submit">Greet</Button>
      </form>
      <p>{greetMsg}</p>
    </main>
  );
}
