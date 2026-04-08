<script lang="ts">
	import { browser } from '$app/environment';
	import { invoke } from '@tauri-apps/api/core';

	type PdfFile = {
		id: string;
		name: string;
		size: number; // en octets
		path: string;
		nbPages: number;
	};

	let files = $state<PdfFile[]>([
		{
			id: '1',
			name: 'contrat-client.pdf',
			size: 248576,
			path: 'C:/mock/contrat-client.pdf',
			nbPages: 6
		},
		{
			id: '2',
			name: 'facture-avril.pdf',
			size: 8990,
			path: 'C:/mock/facture-avril.pdf',
			nbPages: 2
		},
		{
			id: '3',
			name: 'rapport-annuel.pdf',
			size: 1942104,
			path: 'C:/mock/rapport-annuel.pdf',
			nbPages: 18
		}
	]);
	let operationStatus = $state<string | null>(null);
	let operationError = $state<string | null>(null);
	let selectedPages = $state<number[]>([]);
	let isMerging = $state(false);
	let isSplitting = $state(false);

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
				const nbPages = await invoke('get_pdf_page_count', { inputPath: path }).then(
					(result) => (result as { page_count?: number }).page_count ?? 0
				);
				return {
					id: crypto.randomUUID(),
					name,
					size,
					path,
					nbPages
				};
			})
		);
		const uniqueNewFiles = newFiles.filter(
			(newFile) => !files.some((existingFile) => existingFile.path === newFile.path)
		);
		files = [...files, ...uniqueNewFiles];
		selectedPages = [];
	}

	async function runMergePdfs() {
		if (files.length < 2 || isMerging) return;
		if (!browser) return;

		operationStatus = null;
		operationError = null;
		isMerging = true;

		try {
			const { save } = await import('@tauri-apps/plugin-dialog');
			const outputPath = await save({
				defaultPath: 'merged.pdf',
				filters: [{ name: 'PDF', extensions: ['pdf'] }]
			});

			if (!outputPath) {
				return;
			}

			const inputPaths = files.map((file) => file.path);
			const result = await invoke('merge_pdfs', {
				inputPaths,
				outputPath
			});

			operationStatus = (result as { message?: string }).message ?? 'Fusion terminée.';
		} catch (error) {
			operationError = error instanceof Error ? error.message : String(error);
		} finally {
			isMerging = false;
		}
	}

	async function runSplitPdfs() {
		if (files.length !== 1 || isSplitting || selectedPages.length === 0) return;
		if (!browser) return;

		operationStatus = null;
		operationError = null;
		isSplitting = true;

		try {
			const { save } = await import('@tauri-apps/plugin-dialog');
			const outputPath = await save({
				defaultPath: 'split-selected-pages.pdf',
				filters: [{ name: 'PDF', extensions: ['pdf'] }]
			});

			if (!outputPath) {
				return;
			}

			const result = await invoke('split_pdfs', {
				inputPath: files[0].path,
				selectedPages,
				outputPath
			});

			operationStatus = (result as { message?: string }).message ?? 'Split terminé.';
		} catch (error) {
			operationError = error instanceof Error ? error.message : String(error);
		} finally {
			isSplitting = false;
		}
	}

	function selectPage(pageNumber: number) {
		if (selectedPages.includes(pageNumber)) {
			selectedPages = selectedPages.filter((page) => page !== pageNumber);
		} else {
			selectedPages.push(pageNumber);
		}
	}
</script>

{#if files.length > 0}
	<h1>Liste des fichiers PDF:</h1>

	<button
		disabled={files.length < 2}
		class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
		onclick={runMergePdfs}>{isMerging ? 'Fusion en cours...' : 'FUSION'}</button
	>
	<button
		disabled={files.length !== 1 || selectedPages.length === 0}
		class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
		onclick={runSplitPdfs}>{isSplitting ? 'Split en cours...' : 'SPLIT'}</button
	>

	{#if operationStatus}
		<p>{operationStatus}</p>
	{/if}

	{#if operationError}
		<p>{operationError}</p>
	{/if}

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
				{#if files.length === 1}
					<ul>
						{#each Array.from({ length: file.nbPages }, (_, i) => i + 1) as pageNumber (pageNumber)}
							<li>
								Page {pageNumber}
								{#if selectedPages.includes(pageNumber)}
									<button
										class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
										onclick={() => selectPage(pageNumber)}>SELECTED</button
									>
								{:else}
									<button
										class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
										onclick={() => selectPage(pageNumber)}>SELECT</button
									>
								{/if}
							</li>
						{/each}
					</ul>
				{/if}
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
