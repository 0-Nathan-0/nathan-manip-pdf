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

	type Page = {
		pageNumber: number;
		rotation: number;
	};

	let files = $state<PdfFile[]>([
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'contrat-client.pdf',
		// 	size: 248512,
		// 	path: 'mock://contrat-client.pdf',
		// 	nbPages: 3
		// },
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'facture-avril.pdf',
		// 	size: 104320,
		// 	path: 'mock://facture-avril.pdf',
		// 	nbPages: 1
		// },
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'presentation-projet.pdf',
		// 	size: 890112,
		// 	path: 'mock://presentation-projet.pdf',
		// 	nbPages: 8
		// },
		// {
		// 	id: crypto.randomUUID(),
		// 	name: 'rapport-2025.pdf',
		// 	size: 1520430,
		// 	path: 'mock://rapport-2025.pdf',
		// 	nbPages: 12
		// }
	]);
	let operationStatus = $state<string | null>(null);
	let operationError = $state<string | null>(null);
	let selectedPages = $state<Page[]>([]);
	let isMerging = $state(false);
	let isSplitting = $state(false);
	let thumbnails = $state<Record<string, string>>({});
	let isMock = $state(false);

	function thumbnailKey(fileId: string, pageNumber: number) {
		return `${fileId}:${pageNumber}`;
	}

	async function renderPagesForFile(file: PdfFile, pageNumbers: number[]) {
		if (!browser) return;
		if (isMock) {
			const nextThumbnails: Record<string, string> = {};

			for (const pageNumber of pageNumbers) {
				const canvas = document.createElement('canvas');
				canvas.width = 220;
				canvas.height = 300;

				const context = canvas.getContext('2d');
				if (!context) continue;

				context.fillStyle = '#ffffff';
				context.fillRect(0, 0, canvas.width, canvas.height);

				context.fillStyle = '#2563eb';
				context.fillRect(0, 0, canvas.width, 44);

				context.fillStyle = '#ffffff';
				context.font = 'bold 14px sans-serif';
				context.fillText(file.name.slice(0, 20), 10, 28);

				context.fillStyle = '#0f172a';
				context.font = 'bold 28px sans-serif';
				context.fillText(String(pageNumber), 96, 170);

				nextThumbnails[thumbnailKey(file.id, pageNumber)] = canvas.toDataURL('image/png');
			}

			thumbnails = { ...thumbnails, ...nextThumbnails };
		} else {
			try {
				const pdfjs = await import('pdfjs-dist');
				const pdfWorker = await import('pdfjs-dist/build/pdf.worker.min.mjs?url');
				const { readFile } = await import('@tauri-apps/plugin-fs');
				pdfjs.GlobalWorkerOptions.workerSrc = pdfWorker.default;

				const fileData = await readFile(file.path);
				const pdfData = fileData instanceof Uint8Array ? fileData : new Uint8Array(fileData);
				const pdfDoc = await pdfjs.getDocument({ data: pdfData }).promise;
				const nextThumbnails: Record<string, string> = {};

				for (const pageNumber of pageNumbers) {
					try {
						const page = await pdfDoc.getPage(pageNumber);
						const canvas = document.createElement('canvas');
						const context = canvas.getContext('2d');
						if (!context) continue;

						const viewport = page.getViewport({ scale: 0.35 });
						canvas.width = Math.floor(viewport.width);
						canvas.height = Math.floor(viewport.height);

						await page.render({
							canvasContext: context,
							viewport,
							canvas
						}).promise;

						nextThumbnails[thumbnailKey(file.id, pageNumber)] = canvas.toDataURL('image/png');
					} catch (pageError) {
						operationError = pageError instanceof Error ? pageError.message : String(pageError);
					}
				}

				thumbnails = {
					...thumbnails,
					...nextThumbnails
				};
			} catch (error) {
				operationError = error instanceof Error ? error.message : String(error);
			}
		}
	}

	async function refreshThumbnails(currentFiles: PdfFile[]) {
		thumbnails = {};

		if (currentFiles.length === 0) return;

		if (currentFiles.length === 1) {
			const file = currentFiles[0];
			const pages = Array.from({ length: file.nbPages }, (_, i) => i + 1);
			await renderPagesForFile(file, pages);
			return;
		}

		for (const file of currentFiles) {
			await renderPagesForFile(file, [1]);
		}
	}

	async function removeFile(id: string) {
		files = files.filter((file) => file.id !== id);
		selectedPages = [];
		await refreshThumbnails(files);
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
		await refreshThumbnails(files);
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

	async function runSplitPdf() {
		if (files.length !== 1 || isSplitting || selectedPages.length === 0) return;
		if (!browser) return;

		operationStatus = null;
		operationError = null;
		isSplitting = true;

		try {
			const { save } = await import('@tauri-apps/plugin-dialog');
			const outputPath = await save({
				defaultPath: 'splitted.pdf',
				filters: [{ name: 'PDF', extensions: ['pdf'] }]
			});

			if (!outputPath) {
				return;
			}

			const result = await invoke('split_pdf', {
				inputPath: files[0].path,
				selectedPages: selectedPages.map((page) => ({
					pageNumber: page.pageNumber,
					rotation: normalizeRotation(page.rotation)
				})),
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
		const alreadySelected = selectedPages.some((p) => p.pageNumber === pageNumber);

		if (alreadySelected) {
			selectedPages = selectedPages.filter((p) => p.pageNumber !== pageNumber);
		} else {
			selectedPages = [...selectedPages, { pageNumber, rotation: 0 }];
		}
	}

	function movePage(pageNumber: number, direction: -1 | 1) {
		const index = selectedPages.findIndex((p) => p.pageNumber === pageNumber);
		const targetIndex = index + direction;

		if (index < 0 || targetIndex < 0 || targetIndex >= selectedPages.length) {
			return;
		}

		const [page] = selectedPages.splice(index, 1);
		selectedPages.splice(targetIndex, 0, page);
		selectedPages = [...selectedPages];
	}

	function rotatePage(pageNumber: number, angle: -90 | 90) {
		const index = selectedPages.findIndex((p) => p.pageNumber === pageNumber);
		if (index < 0) {
			return;
		}

		selectedPages[index].rotation = selectedPages[index].rotation + angle;
		selectedPages = [...selectedPages];
	}

	function moveFile(id: string, direction: -1 | 1) {
		const index = files.findIndex((file) => file.id === id);
		const targetIndex = index + direction;
		if (index < 0 || targetIndex < 0 || targetIndex >= files.length) {
			return;
		}
		const [file] = files.splice(index, 1);
		files.splice(targetIndex, 0, file);
		files = [...files];
	}

	function normalizeRotation(rotation: number) {
		return ((rotation % 360) + 360) % 360;
	}
</script>

{#if files.length > 0}
	<h1>Liste des fichiers PDF:</h1>

	<button
		type="button"
		disabled={files.length < 2}
		class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
		onclick={runMergePdfs}>{isMerging ? 'Fusion en cours...' : 'FUSION'}</button
	>
	<button
		type="button"
		disabled={files.length !== 1 || selectedPages.length === 0}
		class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
		onclick={runSplitPdf}>{isSplitting ? 'Split en cours...' : 'SPLIT'}</button
	>

	{#if operationStatus}
		<p class="text-red-600">{operationStatus}</p>
	{/if}

	{#if operationError}
		<p class="text-red-600">{operationError}</p>
	{/if}

	{#if selectedPages.length > 0}
		<h2>Ordre des pages sélectionnées</h2>
		<ul>
			{#each selectedPages as page, index (page.pageNumber)}
				<li>
					Page {page.pageNumber}
					<button
						type="button"
						onclick={() => selectPage(page.pageNumber)}
						class="rounded border px-2 py-1 text-sm hover:bg-gray-100 disabled:cursor-not-allowed disabled:opacity-50"
						>X</button
					>
					<button
						type="button"
						onclick={() => movePage(page.pageNumber, -1)}
						hidden={index === 0}
						class="rounded border px-2 py-1 text-sm hover:bg-gray-100 disabled:cursor-not-allowed disabled:opacity-50"
					>
						UP
					</button>
					<button
						type="button"
						onclick={() => movePage(page.pageNumber, 1)}
						hidden={index === selectedPages.length - 1}
						class="rounded border px-2 py-1 text-sm hover:bg-gray-100 disabled:cursor-not-allowed disabled:opacity-50"
					>
						DOWN
					</button>
					<button
						type="button"
						onclick={() => rotatePage(page.pageNumber, 90)}
						class="rounded border px-2 py-1 text-sm hover:bg-gray-100">RIGHT</button
					>
					<button
						type="button"
						onclick={() => rotatePage(page.pageNumber, -90)}
						class="rounded border px-2 py-1 text-sm hover:bg-gray-100">LEFT</button
					>
					{#if thumbnails[thumbnailKey(files[0].id, page.pageNumber)]}
						<div
							class="mt-2 flex h-28 w-24 items-center justify-center overflow-hidden rounded border bg-white"
						>
							<img
								src={thumbnails[thumbnailKey(files[0].id, page.pageNumber)]}
								alt={'Miniature page ' + page.pageNumber}
								class="h-24 w-auto rounded border"
								style:transform={'rotate(' + page.rotation + 'deg)'}
								style:transform-origin="center center"
								style:transition="transform 180ms ease"
							/>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
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
					onclick={() => moveFile(file.id, -1)}
					aria-label={`Monter ${file.name}`}
					hidden={files[0]?.id === file.id}
					class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
				>
					UP</button
				>
				<button
					type="button"
					onclick={() => moveFile(file.id, 1)}
					aria-label={`Descendre ${file.name}`}
					hidden={files[files.length - 1]?.id === file.id}
					class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
				>
					DOWN</button
				>
				{#if files.length > 1 && thumbnails[thumbnailKey(file.id, 1)]}
					<img
						src={thumbnails[thumbnailKey(file.id, 1)]}
						alt={`Miniature page 1 - ${file.name}`}
						class="mt-2 h-24 w-auto rounded border"
					/>
				{/if}
				{#if files.length === 1}
					<ul>
						{#each Array.from({ length: file.nbPages }, (_, i) => i + 1) as pageNumber (pageNumber)}
							<li>
								Page {pageNumber}
								{#if thumbnails[thumbnailKey(file.id, pageNumber)]}
									<img
										src={thumbnails[thumbnailKey(file.id, pageNumber)]}
										alt={`Miniature page ${pageNumber}`}
										class="mt-2 h-24 w-auto rounded border"
									/>
								{/if}

								{#if selectedPages.some((p) => p.pageNumber === pageNumber)}
									<button
										type="button"
										class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
										onclick={() => selectPage(pageNumber)}>SELECTED</button
									>
								{:else}
									<button
										type="button"
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
<button
	type="button"
	class="rounded border px-2 py-1 text-sm hover:bg-gray-100"
	onclick={selectPdfFiles}>AJOUTER PDF</button
>
