use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{mint_to, Mint, MintTo, TokenAccount, TokenInterface}
};
use switchboard_on_demand::accounts::RandomnessAccountData;
use anchor_spl::metadata::{
    Metadata,
    MetadataAccount,
    CreateMetadataAccountsV3,
    CreateMasterEditionV3,
    SignMetadata,
    SetAndVerifySizedCollectionItem,
    create_master_edition_v3,
    create_metadata_accounts_v3,
    sign_metadata,
    set_and_verify_sized_collection_item,
    mpl_token_metadata::types::{
            CollectionDetails,
            Creator, 
            DataV2,
        },
};

declare_id!("2RTh2Y4e2N421EbSnUYTKdGqDHJH7etxZb3VrWDMpNMY");

#[constant]
pub const NAME: &str = "Token Lottery Ticket #";
#[constant]
pub const URI: &str = "Token Lottery";
#[constant]
pub const SYMBOL: &str = "TICKET";

#[program]
pub mod token_lottery {

    use super::*;

    pub fn initialize_config(ctx: Context<InitializeConfig>, start: u64, end: u64, price: u64) -> Result<()> {
        ctx.accounts.token_lottery.bump = ctx.bumps.token_lottery;
        ctx.accounts.token_lottery.lottery_start = start;
        ctx.accounts.token_lottery.lottery_end = end;
        ctx.accounts.token_lottery.price = price;
        ctx.accounts.token_lottery.authority = ctx.accounts.payer.key();
        ctx.accounts.token_lottery.randomness_account = Pubkey::default();

        ctx.accounts.token_lottery.total_tickets = 0;
        ctx.accounts.token_lottery.lottery_pot_amount = 0;
        ctx.accounts.token_lottery.winner_chosen = false;
        Ok(())
    }

    pub fn initialize_lottery(ctx: Context<InitializeLottery>) -> Result<()> {
        
        let signer_seeds: &[&[&[u8]]] = &[&[
            b"collection_mint".as_ref(),
            &[ctx.bumps.collection_mint],
        ]];

        // 1. コレクションNFTをミント
        msg!("Creating mint accounts");

        let mint_to_accounts = MintTo {
            mint: ctx.accounts.collection_mint.to_account_info(),
            to: ctx.accounts.collection_token_account.to_account_info(),
            authority: ctx.accounts.collection_mint.to_account_info(),
        };

        let mint_to_cpi_context = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            mint_to_accounts,
        ).with_signer(&signer_seeds);

        mint_to(
            mint_to_cpi_context,
            1,  // amount: ミントするトークン量（NFTなので1）
        )?;

        // 2. メタデータアカウントを作成
        msg!("Creating metadata accounts");
        let create_metadata_accounts_v3_accounts = CreateMetadataAccountsV3 {
            metadata: ctx.accounts.metadata.to_account_info(),
            mint: ctx.accounts.collection_mint.to_account_info(),
            mint_authority: ctx.accounts.collection_mint.to_account_info(),
            update_authority: ctx.accounts.collection_mint.to_account_info(),
            payer: ctx.accounts.payer.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            rent: ctx.accounts.rent.to_account_info(),
        };

        let data_v2 = DataV2 {
            name: NAME.to_string(),             // NFTの名前
            symbol: SYMBOL.to_string(),         // NFTのシンボル
            uri: URI.to_string(),               // メタデータJSONのURI
            seller_fee_basis_points: 0,         // ロイヤリティ（0 = 0%、10000 = 100%）
            creators: Some(vec![Creator {
                address: ctx.accounts.collection_mint.key(),
                verified: false,                // 署名で検証済みかどうか
                share: 100,                     // ロイヤリティの分配割合（合計100）
            }]),
            collection: None,                   // 所属するコレクション（これ自体がコレクションなのでNone）
            uses: None,                         // 使用回数制限（なし）
        };

        let create_metadata_cpi_context = CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            create_metadata_accounts_v3_accounts,
        ).with_signer(&signer_seeds);

        create_metadata_accounts_v3(
            create_metadata_cpi_context,
            data_v2,
            true,  // is_mutable: メタデータを後から更新可能にする
            true,  // update_authority_is_signer: update_authorityが署名者である
            Some(CollectionDetails::V1 { size: 0 }), // collection_details: コレクションNFTとして設定
        )?;

        // 3. マスターエディションを作成
        {
        msg!("Creating Master edition accounts");
        let create_master_edition_v3_accounts = CreateMasterEditionV3 {
            payer: ctx.accounts.payer.to_account_info(),
            mint: ctx.accounts.collection_mint.to_account_info(),
            edition: ctx.accounts.master_edition.to_account_info(),
            mint_authority: ctx.accounts.collection_mint.to_account_info(),
            update_authority: ctx.accounts.collection_mint.to_account_info(),
            metadata: ctx.accounts.metadata.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            rent: ctx.accounts.rent.to_account_info(),
        };

        let create_master_edition_cpi_context = CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            create_master_edition_v3_accounts,
        ).with_signer(&signer_seeds);

        create_master_edition_v3(
            create_master_edition_cpi_context,
            Some(0),  // max_supply: 最大供給量（0 = 追加ミント不可、None = 無制限）
        )?;
        }
        // 4. クリエイターとして署名（コレクションを検証）
        msg!("Verifying collection");
        
        let sign_metadata_accounts = SignMetadata {
            creator: ctx.accounts.collection_mint.to_account_info(),
            metadata: ctx.accounts.metadata.to_account_info(),
        };

        let sign_metadata_cpi_context = CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            sign_metadata_accounts,
        ).with_signer(&signer_seeds);

        sign_metadata(sign_metadata_cpi_context)?;

        Ok(())
    }

    pub fn buy_ticket(ctx: Context<BuyTicket>) -> Result<()> {
        let clock = Clock::get()?;
        let ticket_name = NAME.to_owned() + ctx.accounts.token_lottery.total_tickets.to_string().as_str();

        require!(
            clock.slot >= ctx.accounts.token_lottery.lottery_start &&
            clock.slot <= ctx.accounts.token_lottery.lottery_end,
            ErrorCode::LotteryNotOpen
        );

        // 1. チケット代金を支払う
        let transfer_accounts = Transfer {
            from: ctx.accounts.payer.to_account_info(),
            to: ctx.accounts.token_lottery.to_account_info(),
        };

        let transfer_cpi_context = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            transfer_accounts,
        );

        transfer(
            transfer_cpi_context,
            ctx.accounts.token_lottery.price,  // amount: 支払うSOLの量
        )?;

        ctx.accounts.token_lottery.lottery_pot_amount += ctx.accounts.token_lottery.price;

        let signer_seeds: &[&[&[u8]]] = &[&[
            b"collection_mint".as_ref(),
            &[ctx.bumps.collection_mint],
        ]];

        // 2. チケットNFTをミント
        let mint_to_accounts = MintTo {
            mint: ctx.accounts.ticket_mint.to_account_info(),
            to: ctx.accounts.destination.to_account_info(),
            authority: ctx.accounts.collection_mint.to_account_info(),
        };

        let mint_to_cpi_context = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            mint_to_accounts,
        ).with_signer(&signer_seeds);

        mint_to(
            mint_to_cpi_context,
            1,  // amount: ミントするトークン量（NFTなので1）
        )?;

        // 3. メタデータアカウントを作成
        let create_metadata_accounts_v3_accounts = CreateMetadataAccountsV3 {
            metadata: ctx.accounts.metadata.to_account_info(),
            mint: ctx.accounts.ticket_mint.to_account_info(),
            mint_authority: ctx.accounts.collection_mint.to_account_info(),
            update_authority: ctx.accounts.collection_mint.to_account_info(),
            payer: ctx.accounts.payer.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            rent: ctx.accounts.rent.to_account_info(),
        };

        let data_v2 = DataV2 {
            name: ticket_name,                  // チケット名（例: "Token Lottery Ticket #0"）
            symbol: SYMBOL.to_string(),         // NFTのシンボル
            uri: URI.to_string(),               // メタデータJSONのURI
            seller_fee_basis_points: 0,         // ロイヤリティ（0%）
            creators: None,                     // クリエイター情報（なし）
            collection: None,                   // コレクションは後でset_and_verify_sized_collection_itemで設定
            uses: None,                         // 使用回数制限（なし）
        };

        let create_metadata_cpi_context = CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            create_metadata_accounts_v3_accounts,
        ).with_signer(&signer_seeds);

        create_metadata_accounts_v3(
            create_metadata_cpi_context,
            data_v2,
            true,  // is_mutable: メタデータを後から更新可能にする
            true,  // update_authority_is_signer: update_authorityが署名者である
            None,  // collection_details: 通常のNFT（コレクションではない）
        )?;

        // 4. マスターエディションを作成
        let create_master_edition_v3_accounts = CreateMasterEditionV3 {
            payer: ctx.accounts.payer.to_account_info(),
            mint: ctx.accounts.ticket_mint.to_account_info(),
            edition: ctx.accounts.master_edition.to_account_info(),
            mint_authority: ctx.accounts.collection_mint.to_account_info(),
            update_authority: ctx.accounts.collection_mint.to_account_info(),
            metadata: ctx.accounts.metadata.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            rent: ctx.accounts.rent.to_account_info(),
        };

        let create_master_edition_cpi_context = CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            create_master_edition_v3_accounts,
        ).with_signer(&signer_seeds);

        create_master_edition_v3(
            create_master_edition_cpi_context,
            Some(0),  // max_supply: 最大供給量（0 = 追加ミント不可）
        )?;

        // 5. NFTをコレクションに追加して検証
        let set_and_verify_sized_collection_item_accounts = SetAndVerifySizedCollectionItem {
            metadata: ctx.accounts.metadata.to_account_info(),
            collection_authority: ctx.accounts.collection_mint.to_account_info(),
            payer: ctx.accounts.payer.to_account_info(),
            update_authority: ctx.accounts.collection_mint.to_account_info(),
            collection_mint: ctx.accounts.collection_mint.to_account_info(),
            collection_metadata: ctx.accounts.collection_metadata.to_account_info(),
            collection_master_edition: ctx.accounts.collection_master_edition.to_account_info(),
        };

        let set_and_verify_cpi_context = CpiContext::new(
            ctx.accounts.token_metadata_program.to_account_info(),
            set_and_verify_sized_collection_item_accounts,
        ).with_signer(&signer_seeds);

        set_and_verify_sized_collection_item(
            set_and_verify_cpi_context,
            None,  // collection_authority_record: コレクション権限の委譲レコード（なし）
        )?;

        ctx.accounts.token_lottery.total_tickets += 1;

        Ok(())
    }

    pub fn commit_winner(ctx: Context<CommitWinner>) -> Result<()> {
        let clock = Clock::get()?;
        let token_lottery = &mut ctx.accounts.token_lottery;
        require!(
            ctx.accounts.payer.key() == token_lottery.authority,
            ErrorCode::NotAuthorized
        );

        let randomness_data = RandomnessAccountData::parse(ctx.accounts.randomness_account_data.data.borrow()).unwrap();

        require!(
            randomness_data.seed_slot == clock.slot - 1,
            ErrorCode::RandomnessAlreadyRevealed
        );

        token_lottery.randomness_account = ctx.accounts.randomness_account_data.key();

        Ok(())
    }

    pub fn reveal_winner(ctx: Context<RevealWinner>) -> Result<()> {
        let clock = Clock::get()?;
        let token_lottery = &mut ctx.accounts.token_lottery;

        require!(
            ctx.accounts.randomness_account_data.key() == token_lottery.randomness_account,
            ErrorCode::IncorrectRandomnessAccount
        );
        require!(
            ctx.accounts.payer.key() == token_lottery.authority,
            ErrorCode::NotAuthorized
        );
        require!(
            clock.slot >= token_lottery.lottery_end,
            ErrorCode::LotteryNotCompleted
        );
        require!(!token_lottery.winner_chosen, ErrorCode::WinnerChosen);
        require!(token_lottery.total_tickets > 0, ErrorCode::NoTicketsSold);

        let randomness_data =
            RandomnessAccountData::parse(ctx.accounts.randomness_account_data.data.borrow()).unwrap();
        let revealed_random_value = randomness_data.get_value(clock.slot)
            .map_err(|_| ErrorCode::RandomnessNotResolved)?;

        msg!("Randomness result: {}", revealed_random_value[0]);
        msg!("Ticket num: {}", token_lottery.total_tickets);

        let randomness_result =
            revealed_random_value[0] as u64 % token_lottery.total_tickets;

        msg!("Winner: {}", randomness_result);

        token_lottery.winning_ticket_id = randomness_result;
        token_lottery.winner_chosen = true;

        Ok(())
    }

    pub fn claim_prize(ctx: Context<ClaimPrize>) -> Result<()> {
        // Check if winner has been chosen
        msg!("Winner chosen: {}", ctx.accounts.token_lottery.winner_chosen);
        require!(ctx.accounts.token_lottery.winner_chosen, ErrorCode::WinnerNotChosen);
        
        // Check if token is a part of the collection
        let collection = ctx.accounts.metadata.collection.as_ref()
            .ok_or(ErrorCode::NoCollection)?;
        require!(collection.verified, ErrorCode::NotVerifiedTicket);
        require!(collection.key == ctx.accounts.collection_mint.key(), ErrorCode::IncorrectTicket);

        let ticket_name = NAME.to_owned() + &ctx.accounts.token_lottery.winning_ticket_id.to_string();
        let metadata_name = ctx.accounts.metadata.name.replace("\u{0}", "");

        msg!("Ticket name: {}", ticket_name);
        msg!("Metadata name: {}", metadata_name);

        // Check if the winner has the winning ticket
        require!(metadata_name == ticket_name, ErrorCode::IncorrectTicket);
        require!(ctx.accounts.destination.amount > 0, ErrorCode::IncorrectTicket);

        let signer_seeds: &[&[&[u8]]] = &[&[
            b"token_lottery".as_ref(),
            &[ctx.accounts.token_lottery.bump],
        ]];

        let transfer_accounts = Transfer {
            from: ctx.accounts.token_lottery.to_account_info(),
            to: ctx.accounts.payer.to_account_info(),
        };

        let transfer_cpi_context = CpiContext::new_with_signer(
            ctx.accounts.system_program.to_account_info(),
            transfer_accounts,
            signer_seeds,
        );

        transfer(
            transfer_cpi_context,
            ctx.accounts.token_lottery.lottery_pot_amount,
        )?;

        ctx.accounts.token_lottery.lottery_pot_amount = 0;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct ClaimPrize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"token_lottery".as_ref()],
        bump = token_lottery.bump,
    )]
    pub token_lottery: Account<'info, TokenLottery>,

    #[account(
        mut,
        seeds = [b"collection_mint".as_ref()],
        bump,
    )]
    pub collection_mint: InterfaceAccount<'info, Mint>,

    #[account(
        seeds = [token_lottery.winning_ticket_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub ticket_mint: InterfaceAccount<'info, Mint>,

    #[account(
        seeds = [b"metadata", token_metadata_program.key().as_ref(), ticket_mint.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    pub metadata: Account<'info, MetadataAccount>,

    #[account(
        associated_token::mint = ticket_mint,
        associated_token::authority = payer,
        associated_token::token_program = token_program,
    )]
    pub destination: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), collection_mint.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    pub collection_metadata: Account<'info, MetadataAccount>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub token_metadata_program: Program<'info, Metadata>,
}

#[derive(Accounts)]
pub struct CommitWinner<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"token_lottery".as_ref()],
        bump = token_lottery.bump,
    )]
    pub token_lottery: Account<'info, TokenLottery>,

    /// CHECK: The account's data is validated manually within the handler.
    pub randomness_account_data: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevealWinner<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"token_lottery".as_ref()],
        bump = token_lottery.bump,
    )]
    pub token_lottery: Account<'info, TokenLottery>,

    /// CHECK: The account's data is validated manually within the handler.
    pub randomness_account_data: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BuyTicket<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"token_lottery".as_ref()],
        bump = token_lottery.bump
    )]
    pub token_lottery: Account<'info, TokenLottery>,

    #[account(
        init,
        payer = payer,
        seeds = [token_lottery.total_tickets.to_le_bytes().as_ref()],
        bump,
        mint::decimals = 0,
        mint::authority = collection_mint,
        mint::freeze_authority = collection_mint,
        mint::token_program = token_program
    )]
    pub ticket_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = ticket_mint,
        associated_token::authority = payer,
        associated_token::token_program = token_program,
    )]
    pub destination: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), 
        ticket_mint.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    /// CHECK: This account will be initialized by the metaplex program
    pub metadata: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), 
            ticket_mint.key().as_ref(), b"edition"],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    /// CHECK: This account will be initialized by the metaplex program
    pub master_edition: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), collection_mint.key().as_ref()],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    /// CHECK: This account will be initialized by the metaplex program
    pub collection_metadata: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"metadata", token_metadata_program.key().as_ref(), 
            collection_mint.key().as_ref(), b"edition"],
        bump,
        seeds::program = token_metadata_program.key(),
    )]
    /// CHECK: This account will be initialized by the metaplex program
    pub collection_master_edition: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [b"collection_mint".as_ref()],
        bump,
    )]
    pub collection_mint: InterfaceAccount<'info, Mint>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub token_metadata_program: Program<'info, Metadata>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + TokenLottery::INIT_SPACE,
        // Challenge: Make this be able to run more than 1 lottery at a time
        seeds = [b"token_lottery".as_ref()],
        bump
    )]
    pub token_lottery: Box<Account<'info, TokenLottery>>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeLottery<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        mint::decimals = 0,
        mint::authority = collection_mint,
        mint::freeze_authority = collection_mint,
        seeds = [b"collection_mint".as_ref()],
        bump,
    )]
    pub collection_mint: Box<InterfaceAccount<'info, Mint>>,

    /// CHECK: This account will be initialized by the metaplex program
    #[account(mut)]
    pub metadata: UncheckedAccount<'info>,

    /// CHECK: This account will be initialized by the metaplex program
    #[account(mut)]
    pub master_edition: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = payer,
        seeds = [b"collection_token_account".as_ref()],
        bump,
        token::mint = collection_mint,
        token::authority = collection_token_account
    )]
    pub collection_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub token_metadata_program: Program<'info, Metadata>,
    pub rent: Sysvar<'info, Rent>,
}

#[account]
#[derive(InitSpace)]
pub struct TokenLottery {
    pub bump: u8,
    pub winning_ticket_id: u64,
    pub winner_chosen: bool,
    pub lottery_start: u64,
    pub lottery_end: u64,
    pub lottery_pot_amount: u64,
    pub total_tickets: u64,
    pub price: u64,
    pub randomness_account: Pubkey,
    pub authority: Pubkey,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Incorrect randomness account")]
    IncorrectRandomnessAccount,
    #[msg("Lottery not completed")]
    LotteryNotCompleted,
    #[msg("Lottery is not open")]
    LotteryNotOpen,
    #[msg("Not authorized")]
    NotAuthorized,
    #[msg("Randomness already revealed")]
    RandomnessAlreadyRevealed,
    #[msg("Randomness not resolved")]
    RandomnessNotResolved,
    #[msg("Winner not chosen")]
    WinnerNotChosen,
    #[msg("Winner already chosen")]
    WinnerChosen,
    #[msg("Ticket is not verified")]
    NotVerifiedTicket,
    #[msg("Incorrect ticket")]
    IncorrectTicket,
    #[msg("No tickets sold")]
    NoTicketsSold,
    #[msg("Ticket has no collection")]
    NoCollection,
}