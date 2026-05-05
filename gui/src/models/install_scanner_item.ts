import {z} from "zod";

export const InstallScannerItemSchema = z.object({
  title: z.string(),
  dirs: z.array(z.string()),
  icon: z.string(),
})

export type InstallScannerItem = z.infer<typeof InstallScannerItemSchema>
