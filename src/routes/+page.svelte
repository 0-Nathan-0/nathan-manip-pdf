<script lang="ts">
	import { browser } from '$app/environment';

	type PdfFile = {
		id: string;
		name: string;
		size: number; // en octets
		path: string;
	};

	let files = $state<PdfFile[]>([]);

	function removeFile(id: string) {
		files = files.filter((file) => file.id !== id);
	}

	function goDown(id: string) {
		for (let i = 0; i < files.length - 1; i++) {
			if (files[i].id === id) {
				const tmp = files[i];
				files[i] = files[i + 1];
				files[i + 1] = tmp;
				return;
			}
		}
	}

	function goUp(id: string) {
		for (let i = 1; i < files.length; i++) {
			if (files[i].id === id) {
				const tmp = files[i];
				files[i] = files[i - 1];
				files[i - 1] = tmp;
				return;
			}
		}
	}

	async function selectPdfFiles() {
		if (!browser) return;

		const { open } = await import('@tauri-apps/plugin-dialog');
		const { stat } = await import('@tauri-apps/plugin-fs');

		const selected = await open({
			multiple: true,
			filters: [{ name: 'PDF', extensions: ['pdf'] }]
		});

		if (!selected) return;

		const paths = Array.isArray(selected) ? selected : [selected];
		const newFiles: PdfFile[] = await Promise.all(
			paths.map(async (path) => {
				const normalized = path.replaceAll('\\', '/');
				const name = normalized.split('/').pop() ?? 'unknown.pdf';

				const fileInfo = await stat(path);
				const size = typeof fileInfo.size === 'number' ? fileInfo.size : 0;
				return {
					id: crypto.randomUUID(),
					name,
					size,
					path
				};
			})
		);
		const uniqueNewFiles = newFiles.filter(
			(newFile) => !files.some((existingFile) => existingFile.path === newFile.path)
		);
		files = [...files, ...uniqueNewFiles];
	}
</script>

{#if files.length > 0}
	<h1>Liste des fichiers PDF:</h1>

	<button disabled={files.length < 2} class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
		>FUSION</button
	>

	<ul>
		{#each files as file (file.id)}
			<li>
				<strong>{file.name}</strong> - {file.size} Octets
				<button
					type="button"
					onclick={() => removeFile(file.id)}
					aria-label={`Supprimer ${file.name}`}
					class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
				>
					X</button
				>
				<button
					type="button"
					onclick={() => goUp(file.id)}
					aria-label={`Monter ${file.name}`}
					hidden={files[0]?.id === file.id}
					class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
				>
					UP</button
				>
				<button
					type="button"
					onclick={() => goDown(file.id)}
					aria-label={`Descendre ${file.name}`}
					hidden={files[files.length - 1]?.id === file.id}
					class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
				>
					DOWN</button
				>
			</li>
		{/each}
	</ul>
{:else}
	<p>Aucun fichier pour le moment.</p>
{/if}
<p>Nb fichiers: {files.length}</p>
<button class="rounded border px-2 py-1 text-sm hover:bg-gray-100" onclick={selectPdfFiles}
	>AJOUTER PDF</button
>
